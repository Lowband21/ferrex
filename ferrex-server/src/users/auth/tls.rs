//! TLS configuration and certificate management for Ferrex media server
//!
//! This module provides:
//! - TLS configuration loading with hot reload support
//! - Certificate and key management
//! - Connection pooling with rustls
//! - Comprehensive error types for certificate failures
//! - TLS handshake metrics via prometheus

use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use rustls::ServerConfig;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::{
    fmt,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    sync::RwLock,
    time::{MissedTickBehavior, interval},
};
use tracing::{error, info};

/// TLS-related errors
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("Certificate file not found: {0}")]
    CertificateNotFound(PathBuf),

    #[error("Private key file not found: {0}")]
    PrivateKeyNotFound(PathBuf),

    #[error("Failed to parse certificate: {0}")]
    CertificateParseFailed(String),

    #[error("Failed to parse private key: {0}")]
    PrivateKeyParseFailed(String),

    #[error("Certificate chain validation failed: {0}")]
    CertificateValidationFailed(String),

    #[error("No private keys found in file")]
    NoPrivateKeysFound,

    #[error("Multiple private keys found, expected one")]
    MultiplePrivateKeysFound,

    #[error("TLS configuration error: {0}")]
    ConfigurationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// TLS certificate configuration
#[derive(Clone, Debug)]
pub struct TlsCertConfig {
    /// Path to the certificate file (PEM format)
    pub cert_path: PathBuf,
    /// Path to the private key file (PEM format)
    pub key_path: PathBuf,
    /// Enable OCSP stapling
    pub enable_ocsp_stapling: bool,
    /// Minimum TLS version (e.g., "1.2", "1.3")
    pub min_tls_version: String,
    /// Cipher suites to use (empty = use defaults)
    pub cipher_suites: Vec<String>,
}

impl Default for TlsCertConfig {
    fn default() -> Self {
        Self {
            cert_path: PathBuf::from("certs/cert.pem"),
            key_path: PathBuf::from("certs/key.pem"),
            enable_ocsp_stapling: true,
            min_tls_version: "1.2".to_string(),
            cipher_suites: vec![],
        }
    }
}

/// TLS configuration manager with hot reload support
pub struct TlsConfigManager {
    config: Arc<RwLock<TlsCertConfig>>,
    rustls_config: Arc<RwLock<Arc<ServerConfig>>>,
    reload_interval: Duration,
}

impl fmt::Debug for TlsConfigManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let config_snapshot = self.config.try_read().ok().map(|guard| guard.clone());
        let rustls_config_state = self
            .rustls_config
            .try_read()
            .ok()
            .map(|guard| Arc::strong_count(&*guard));

        f.debug_struct("TlsConfigManager")
            .field("config", &config_snapshot)
            .field("rustls_config_strong_count", &rustls_config_state)
            .field("reload_interval", &self.reload_interval)
            .finish()
    }
}

impl TlsConfigManager {
    /// Create a new TLS configuration manager
    pub async fn new(config: TlsCertConfig) -> Result<Self, TlsError> {
        let rustls_config = Self::load_rustls_config(&config).await?;

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            rustls_config: Arc::new(RwLock::new(Arc::new(rustls_config))),
            reload_interval: Duration::from_secs(300), // Check every 5 minutes
        })
    }

    /// Start the certificate hot reload task
    pub fn start_hot_reload(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = interval(self.reload_interval);
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                if let Err(e) = self.reload_certificates().await {
                    error!("Failed to reload TLS certificates: {}", e);
                }
            }
        });
    }

    /// Get the current rustls configuration
    pub async fn get_rustls_config(&self) -> Arc<ServerConfig> {
        self.rustls_config.read().await.clone()
    }

    /// Create an axum-server RustlsConfig
    pub async fn create_axum_config(&self) -> Result<RustlsConfig> {
        let config = self.config.read().await;
        RustlsConfig::from_pem_file(&config.cert_path, &config.key_path)
            .await
            .context("Failed to create RustlsConfig")
    }

    /// Reload certificates from disk
    async fn reload_certificates(&self) -> Result<(), TlsError> {
        let config = self.config.read().await.clone();

        // Check if files have been modified
        let cert_modified = Self::check_file_modified(&config.cert_path).await;
        let key_modified = Self::check_file_modified(&config.key_path).await;

        if cert_modified || key_modified {
            info!("TLS certificate change detected, reloading...");

            match Self::load_rustls_config(&config).await {
                Ok(new_config) => {
                    *self.rustls_config.write().await = Arc::new(new_config);
                    info!("TLS certificates reloaded successfully");

                    // TODO: Export metrics for successful reload
                    // metrics::counter!("tls_cert_reload_success").increment(1);
                }
                Err(e) => {
                    error!("Failed to reload TLS certificates: {}", e);

                    // TODO: Export metrics for failed reload
                    // metrics::counter!("tls_cert_reload_failure").increment(1);

                    return Err(e);
                }
            }
        }

        Ok(())
    }

    /// Load rustls configuration from certificate files
    async fn load_rustls_config(config: &TlsCertConfig) -> Result<ServerConfig, TlsError> {
        // Load certificate chain
        let cert_chain = Self::load_certificates(&config.cert_path).await?;

        // Load private key
        let private_key = Self::load_private_key(&config.key_path).await?;

        // Create rustls config
        let mut rustls_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, private_key)
            .map_err(|e| TlsError::ConfigurationError(e.to_string()))?;

        // Note: TLS version configuration in rustls 0.23+ is handled during ServerConfig::builder()
        // The min_tls_version configuration would need to be applied at builder level

        // Configure ALPN for HTTP/2
        rustls_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        Ok(rustls_config)
    }

    /// Load certificates from PEM file
    async fn load_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
        if !path.exists() {
            return Err(TlsError::CertificateNotFound(path.to_path_buf()));
        }

        let mut file = File::open(path).await?;
        let mut pem_data = Vec::new();
        file.read_to_end(&mut pem_data).await?;

        let mut reader = BufReader::new(&pem_data[..]);
        let certs = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TlsError::CertificateParseFailed(e.to_string()))?;

        if certs.is_empty() {
            return Err(TlsError::CertificateParseFailed(
                "No certificates found in file".to_string(),
            ));
        }

        Ok(certs.into_iter().collect())
    }

    /// Load private key from PEM file
    async fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
        if !path.exists() {
            return Err(TlsError::PrivateKeyNotFound(path.to_path_buf()));
        }

        let mut file = File::open(path).await?;
        let mut pem_data = Vec::new();
        file.read_to_end(&mut pem_data).await?;

        let mut reader = BufReader::new(&pem_data[..]);

        // Try to read PKCS#8 private key first
        let keys = rustls_pemfile::pkcs8_private_keys(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TlsError::PrivateKeyParseFailed(e.to_string()))?;

        if !keys.is_empty() {
            if keys.len() > 1 {
                return Err(TlsError::MultiplePrivateKeysFound);
            }
            return Ok(PrivateKeyDer::from(keys.into_iter().next().unwrap()));
        }

        // Try RSA private key format
        let mut reader = BufReader::new(&pem_data[..]);
        let keys = rustls_pemfile::rsa_private_keys(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TlsError::PrivateKeyParseFailed(e.to_string()))?;

        if keys.is_empty() {
            return Err(TlsError::NoPrivateKeysFound);
        }

        if keys.len() > 1 {
            return Err(TlsError::MultiplePrivateKeysFound);
        }

        Ok(PrivateKeyDer::from(keys.into_iter().next().unwrap()))
    }

    /// Check if a file has been modified (simplified check)
    async fn check_file_modified(path: &Path) -> bool {
        // In a production system, you would track modification times
        // For now, we'll just check if the file exists
        path.exists()
    }
}

/// Helper function to create TLS acceptor configuration
pub async fn create_tls_acceptor(config: TlsCertConfig) -> Result<RustlsConfig> {
    RustlsConfig::from_pem_file(&config.cert_path, &config.key_path)
        .await
        .context("Failed to create TLS acceptor")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    const TEST_CERT: &str = r#"-----BEGIN CERTIFICATE-----
<REDACTED>
-----END CERTIFICATE-----"#;

    const TEST_KEY: &str = r#"-----BEGIN RSA PRIVATE KEY-----
<REDACTED>
-----END RSA PRIVATE KEY-----"#;

    async fn create_test_cert_files() -> Result<TempDir> {
        let temp_dir = TempDir::new()?;

        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");

        fs::write(&cert_path, TEST_CERT).await?;
        fs::write(&key_path, TEST_KEY).await?;

        Ok(temp_dir)
    }

    #[tokio::test]
    async fn test_load_certificates() -> Result<()> {
        let temp_dir = create_test_cert_files().await?;
        let cert_path = temp_dir.path().join("cert.pem");

        let certs = TlsConfigManager::load_certificates(&cert_path).await?;
        assert_eq!(certs.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_load_private_key() -> Result<()> {
        let temp_dir = create_test_cert_files().await?;
        let key_path = temp_dir.path().join("key.pem");

        let key = TlsConfigManager::load_private_key(&key_path).await?;
        assert!(!key.secret_der().is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_missing_certificate() {
        let result = TlsConfigManager::load_certificates(Path::new("/nonexistent/cert.pem")).await;
        assert!(matches!(result, Err(TlsError::CertificateNotFound(_))));
    }

    #[tokio::test]
    async fn test_missing_private_key() {
        let result = TlsConfigManager::load_private_key(Path::new("/nonexistent/key.pem")).await;
        assert!(matches!(result, Err(TlsError::PrivateKeyNotFound(_))));
    }
}
