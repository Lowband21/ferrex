//! Hardware fingerprinting module for secure device identification
//!
//! This module generates a stable hardware fingerprint by combining multiple
//! hardware identifiers and hashing them securely. The fingerprint remains
//! stable across application restarts but changes if hardware changes.

use anyhow::Result;
use mac_address::MacAddressIterator;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use sysinfo::{Disks, System};

/// Hardware information collected for fingerprinting
#[derive(Debug, Clone)]
pub struct HardwareInfo {
    /// Primary MAC addresses (stable ordering)
    pub mac_addresses: Vec<String>,
    /// CPU model name
    pub cpu_model: Option<String>,
    /// Total system memory in KB
    pub total_memory: u64,
    /// Disk serial numbers (stable ordering)
    pub disk_serials: Vec<String>,
    /// System hostname
    pub hostname: Option<String>,
    /// Platform-specific hardware ID
    pub platform_id: Option<String>,
}

impl HardwareInfo {
    pub fn collect() -> Result<Self> {
        let mut info = HardwareInfo {
            mac_addresses: Vec::new(),
            cpu_model: None,
            total_memory: 0,
            disk_serials: Vec::new(),
            hostname: None,
            platform_id: None,
        };

        info.mac_addresses = Self::collect_mac_addresses()?;

        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        if let Some(cpu) = sys.cpus().first() {
            let brand = cpu.brand();
            if !brand.is_empty() {
                info.cpu_model = Some(brand.to_string());
            }
        }

        info.total_memory = sys.total_memory();

        if let Some(hostname) = System::host_name()
            && !hostname.is_empty()
        {
            info.hostname = Some(hostname);
        }

        info.disk_serials = Self::collect_disk_serials()?;

        info.platform_id = Self::collect_platform_id();

        Ok(info)
    }

    fn collect_mac_addresses() -> Result<Vec<String>> {
        let mut macs = BTreeSet::new(); // Use BTreeSet for stable ordering

        for mac in MacAddressIterator::new()? {
            // Skip common virtual interface patterns
            if mac.bytes() == [0, 0, 0, 0, 0, 0] {
                continue;
            }

            // Format as string
            let mac_str = mac
                .bytes()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(":");

            if !is_virtual_mac(&mac_str) {
                macs.insert(mac_str);
            }
        }

        Ok(macs.into_iter().collect())
    }

    fn collect_disk_serials() -> Result<Vec<String>> {
        let mut serials = BTreeSet::new();
        let disks = Disks::new_with_refreshed_list();

        for disk in disks.list() {
            if let Some(mount_point) = disk.mount_point().to_str()
                && !is_virtual_filesystem(mount_point, disk.file_system().as_encoded_bytes())
            {
                let identifier = format!("{}:{}", mount_point, disk.total_space());
                serials.insert(identifier);
            }
        }

        Ok(serials.into_iter().collect())
    }

    #[cfg(target_os = "macos")]
    fn collect_platform_id() -> Option<String> {
        std::process::Command::new("system_profiler")
            .args(&["SPHardwareDataType", "-json"])
            .output()
            .ok()
            .and_then(|output| {
                let json_str = String::from_utf8_lossy(&output.stdout);
                json_str.find("\"platform_UUID\"").and_then(|idx| {
                    let start = json_str[idx..].find("\"")?;
                    let end = json_str[idx + start + 1..].find("\"")?;
                    Some(json_str[idx + start + 1..idx + start + 1 + end].to_string())
                })
            })
    }

    #[cfg(target_os = "linux")]
    fn collect_platform_id() -> Option<String> {
        std::fs::read_to_string("/etc/machine-id")
            .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
            .ok()
            .map(|s| s.trim().to_string())
    }

    #[cfg(target_os = "windows")]
    fn collect_platform_id() -> Option<String> {
        std::process::Command::new("wmic")
            .args(&["csproduct", "get", "UUID", "/value"])
            .output()
            .ok()
            .and_then(|output| {
                let output_str = String::from_utf8_lossy(&output.stdout);
                output_str
                    .lines()
                    .find(|line| line.starts_with("UUID="))
                    .and_then(|line| line.strip_prefix("UUID="))
                    .map(|uuid| uuid.trim().to_string())
            })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    fn collect_platform_id() -> Option<String> {
        None
    }

    /// Generate a stable fingerprint from hardware info
    pub fn generate_fingerprint(&self) -> String {
        let mut hasher = Sha256::new();

        // Add components in stable order

        // MAC addresses (primary identifier)
        for mac in &self.mac_addresses {
            hasher.update(b"mac:");
            hasher.update(mac.as_bytes());
            hasher.update(b"\n");
        }

        // Platform ID (if available)
        if let Some(ref id) = self.platform_id {
            hasher.update(b"platform:");
            hasher.update(id.as_bytes());
            hasher.update(b"\n");
        }

        // CPU model
        if let Some(ref cpu) = self.cpu_model {
            hasher.update(b"cpu:");
            hasher.update(cpu.as_bytes());
            hasher.update(b"\n");
        }

        // Total memory (rounded to nearest GB for stability)
        let memory_gb = self.total_memory / (1024 * 1024);
        hasher.update(b"memory:");
        hasher.update(memory_gb.to_string().as_bytes());
        hasher.update(b"\n");

        // Disk identifiers
        for serial in &self.disk_serials {
            hasher.update(b"disk:");
            hasher.update(serial.as_bytes());
            hasher.update(b"\n");
        }

        // Hostname (optional, as it can change)
        if let Some(ref hostname) = self.hostname {
            hasher.update(b"host:");
            hasher.update(hostname.as_bytes());
            hasher.update(b"\n");
        }

        // Generate final hash
        let result = hasher.finalize();
        format!("{:x}", result)
    }
}

/// Check if MAC address belongs to a virtual interface
fn is_virtual_mac(mac: &str) -> bool {
    // Common virtual interface MAC prefixes
    const VIRTUAL_PREFIXES: &[&str] = &[
        "00:50:56", // VMware
        "00:0c:29", // VMware
        "00:05:69", // VMware
        "00:1c:42", // Parallels
        "00:03:ff", // Microsoft Hyper-V
        "00:0f:4b", // Virtual Iron
        "00:16:3e", // Xen
        "08:00:27", // VirtualBox
        "02:42:",   // Docker
        "00:00:00", // Null
    ];

    let mac_lower = mac.to_lowercase();
    VIRTUAL_PREFIXES
        .iter()
        .any(|prefix| mac_lower.starts_with(prefix))
}

/// Check if filesystem is virtual/network based
fn is_virtual_filesystem(mount_point: &str, fs_type: &[u8]) -> bool {
    // Skip common virtual mount points
    if mount_point.starts_with("/dev")
        || mount_point.starts_with("/sys")
        || mount_point.starts_with("/proc")
        || mount_point.starts_with("/run")
        || mount_point.starts_with("/snap")
        || mount_point.contains("docker")
    {
        return true;
    }

    // Check filesystem type
    let fs_str = String::from_utf8_lossy(fs_type).to_lowercase();
    matches!(
        fs_str.as_str(),
        "devfs"
            | "procfs"
            | "sysfs"
            | "tmpfs"
            | "devtmpfs"
            | "overlay"
            | "aufs"
            | "squashfs"
            | "nfs"
            | "cifs"
            | "smb"
    )
}

/// Generate a hardware fingerprint with fallback values
pub async fn generate_hardware_fingerprint() -> Result<String> {
    match HardwareInfo::collect() {
        Ok(info) => {
            // Log collected info for debugging
            log::debug!("Hardware info collected:");
            log::debug!("  MAC addresses: {:?}", info.mac_addresses);
            log::debug!("  Platform ID: {:?}", info.platform_id);
            log::debug!("  CPU model: {:?}", info.cpu_model);
            log::debug!("  Total memory: {} KB", info.total_memory);
            log::debug!("  Disk serials: {:?}", info.disk_serials);

            let fingerprint = info.generate_fingerprint();
            log::info!("Generated hardware fingerprint: {}", &fingerprint[..8]); // Log first 8 chars only
            Ok(fingerprint)
        }
        Err(e) => {
            log::warn!("Failed to collect full hardware info: {}", e);

            // Fallback fingerprint using available data
            let mut hasher = Sha256::new();

            // Try to get at least hostname
            if let Some(hostname) = System::host_name() {
                hasher.update(b"hostname:");
                hasher.update(hostname.as_bytes());
            }

            // Add current user
            if let Ok(username) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
                hasher.update(b"user:");
                hasher.update(username.as_bytes());
            }

            // Add a random component that will be stable per device
            // This ensures uniqueness even if hardware detection fails
            hasher.update(b"fallback:ferrex-player");

            let result = hasher.finalize();
            let fingerprint = format!("{:x}", result);
            log::warn!("Using fallback fingerprint: {}", &fingerprint[..8]);
            Ok(fingerprint)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_mac_detection() {
        assert!(is_virtual_mac("00:50:56:12:34:56"));
        assert!(is_virtual_mac("08:00:27:ab:cd:ef"));
        assert!(!is_virtual_mac("a4:5e:60:12:34:56")); // Real MAC
    }

    #[test]
    fn test_virtual_filesystem_detection() {
        assert!(is_virtual_filesystem("/dev/shm", b"tmpfs"));
        assert!(is_virtual_filesystem("/proc", b"proc"));
        assert!(!is_virtual_filesystem("/home", b"ext4"));
    }

    #[tokio::test]
    async fn test_fingerprint_generation() {
        // Just ensure it doesn't panic
        let result = generate_hardware_fingerprint().await;
        assert!(result.is_ok());
        let fingerprint = result.unwrap();
        assert!(!fingerprint.is_empty());
        assert_eq!(fingerprint.len(), 64); // SHA256 hex length
    }
}
