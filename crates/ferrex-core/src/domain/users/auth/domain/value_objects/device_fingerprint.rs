use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use thiserror::Error;

/// Errors that can occur when working with device fingerprints
#[derive(Debug, Error)]
pub enum DeviceFingerprintError {
    #[error("Invalid fingerprint format")]
    InvalidFormat,

    #[error("Missing required hardware information")]
    MissingHardwareInfo,
}

/// Hardware-based device fingerprint
///
/// This value object represents a device's unique fingerprint based on:
/// - Hardware identifiers (CPU, motherboard, etc.)
/// - Operating system information
/// - Network interfaces
///
/// The fingerprint is designed to be stable across application restarts
/// but may change with significant hardware changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceFingerprint {
    /// The computed fingerprint hash
    hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FingerprintComponents {
    /// Operating system name and version
    os_info: String,

    /// CPU model and core count
    cpu_info: Option<String>,

    /// Primary MAC address (if available)
    mac_address: Option<String>,

    /// Machine ID (Linux) or similar platform identifier
    machine_id: Option<String>,

    /// Hostname (hashed for privacy)
    hostname_hash: Option<String>,
}

impl DeviceFingerprint {
    /// Create a new device fingerprint from hardware information
    pub fn new(
        os_info: String,
        cpu_info: Option<String>,
        mac_address: Option<String>,
        machine_id: Option<String>,
        hostname: Option<String>,
    ) -> Result<Self, DeviceFingerprintError> {
        // Ensure we have at least some hardware information
        if os_info.is_empty()
            && cpu_info.is_none()
            && mac_address.is_none()
            && machine_id.is_none()
        {
            return Err(DeviceFingerprintError::MissingHardwareInfo);
        }

        let hostname_hash = hostname.map(|h| {
            let mut hasher = Sha256::new();
            hasher.update(h.as_bytes());
            format!("{:x}", hasher.finalize())
        });

        let components = FingerprintComponents {
            os_info,
            cpu_info,
            mac_address,
            machine_id,
            hostname_hash,
        };

        let hash = Self::compute_hash(&components);

        Ok(Self { hash })
    }

    /// Create from a known hash (for deserialization)
    pub fn from_hash(hash: String) -> Result<Self, DeviceFingerprintError> {
        if hash.is_empty() || hash.len() != 64 {
            // SHA256 produces 64 hex chars
            return Err(DeviceFingerprintError::InvalidFormat);
        }

        Ok(Self { hash })
    }

    /// Compute the fingerprint hash from components
    fn compute_hash(components: &FingerprintComponents) -> String {
        let mut hasher = Sha256::new();

        // Add components in a deterministic order
        hasher.update(b"os:");
        hasher.update(components.os_info.as_bytes());

        if let Some(cpu) = &components.cpu_info {
            hasher.update(b"|cpu:");
            hasher.update(cpu.as_bytes());
        }

        if let Some(mac) = &components.mac_address {
            hasher.update(b"|mac:");
            hasher.update(mac.as_bytes());
        }

        if let Some(machine_id) = &components.machine_id {
            hasher.update(b"|mid:");
            hasher.update(machine_id.as_bytes());
        }

        if let Some(hostname) = &components.hostname_hash {
            hasher.update(b"|host:");
            hasher.update(hostname.as_bytes());
        }

        format!("{:x}", hasher.finalize())
    }

    /// Get the fingerprint hash
    pub fn as_str(&self) -> &str {
        &self.hash
    }
}

impl fmt::Display for DeviceFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Only show first 8 chars for privacy
        let preview = if self.hash.len() > 8 {
            &self.hash[..8]
        } else {
            &self.hash
        };
        write!(f, "{}...", preview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_creation() {
        let fp = DeviceFingerprint::new(
            "Linux 5.15".to_string(),
            Some("Intel i7-8700K".to_string()),
            Some("00:11:22:33:44:55".to_string()),
            Some("machine123".to_string()),
            Some("myhost".to_string()),
        )
        .unwrap();

        assert!(!fp.hash.is_empty());
        assert_eq!(fp.hash.len(), 64); // SHA256 hex length
    }

    #[test]
    fn test_fingerprint_deterministic() {
        let fp1 = DeviceFingerprint::new(
            "Linux 5.15".to_string(),
            Some("Intel i7-8700K".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let fp2 = DeviceFingerprint::new(
            "Linux 5.15".to_string(),
            Some("Intel i7-8700K".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(fp1.hash, fp2.hash);
    }

    #[test]
    fn test_missing_hardware_info() {
        let result =
            DeviceFingerprint::new(String::new(), None, None, None, None);

        assert!(matches!(
            result,
            Err(DeviceFingerprintError::MissingHardwareInfo)
        ));
    }
}
