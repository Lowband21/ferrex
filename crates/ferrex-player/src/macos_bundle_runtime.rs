//! Early runtime path setup for the self-contained macOS application bundle.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
struct BundleRuntimePaths {
    vulkan_icd: PathBuf,
}

fn bundle_runtime_paths(executable: &Path) -> Option<BundleRuntimePaths> {
    let macos = executable.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }

    let vulkan_icd = contents.join("Resources/vulkan/icd.d/MoltenVK_icd.json");
    if !vulkan_icd.is_file() {
        return None;
    }
    Some(BundleRuntimePaths { vulkan_icd })
}

/// Configure dynamic runtime discovery before any worker threads are created.
///
/// Developer launches are unchanged: variables are written only when the
/// current executable is inside a complete `Contents/MacOS` app layout.
#[cfg(target_os = "macos")]
pub fn configure() {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let Some(paths) = bundle_runtime_paths(&executable) else {
        return;
    };
    // SAFETY: `main` calls this as its first operation, before application,
    // logging, or mpv threads exist. No concurrent environment access can
    // have been initiated by Ferrex at this point.
    unsafe {
        std::env::set_var("VK_ICD_FILENAMES", paths.vulkan_icd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir() -> PathBuf {
        let nonce = format!(
            "ferrex-macos-runtime-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        );
        std::env::temp_dir().join(nonce)
    }

    #[test]
    fn recognizes_only_complete_app_bundle_layout() {
        let root = unique_temp_dir();
        let contents = root.join("Ferrex Player.app/Contents");
        let executable = contents.join("MacOS/ferrex-player");
        std::fs::create_dir_all(executable.parent().expect("MacOS parent"))
            .expect("create MacOS");

        assert_eq!(bundle_runtime_paths(&executable), None);

        let icd = contents.join("Resources/vulkan/icd.d/MoltenVK_icd.json");
        std::fs::create_dir_all(icd.parent().expect("ICD parent"))
            .expect("create ICD dir");
        std::fs::write(&icd, b"{}").expect("create ICD");

        assert_eq!(
            bundle_runtime_paths(&executable),
            Some(BundleRuntimePaths { vulkan_icd: icd })
        );
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn rejects_developer_executable_layout() {
        assert_eq!(
            bundle_runtime_paths(Path::new("/tmp/target/debug/ferrex-player")),
            None
        );
    }
}
