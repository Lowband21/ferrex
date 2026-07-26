//! Early runtime path setup for the self-contained macOS application bundle.

use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use std::ffi::CString;
#[cfg(target_os = "macos")]
use std::os::raw::{c_char, c_int, c_void};
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
struct BundleRuntimePaths {
    gstreamer_plugins: PathBuf,
    gstreamer_scanner: PathBuf,
    gio_modules: PathBuf,
    libsoup: PathBuf,
    ca_bundle: PathBuf,
    vulkan_icd: Option<PathBuf>,
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

    let gstreamer_plugins = contents.join("Resources/gstreamer-1.0");
    let gstreamer_scanner = contents.join("Helpers/gst-plugin-scanner");
    let gio_modules = contents.join("Resources/gio/modules");
    let libsoup = contents.join("Frameworks/libsoup-3.0.0.dylib");
    let ca_bundle = contents.join("Resources/tls/cacert.pem");
    if !gstreamer_plugins.is_dir()
        || !gstreamer_scanner.is_file()
        || !gio_modules.is_dir()
        || !libsoup.is_file()
        || !ca_bundle.is_file()
    {
        return None;
    }
    let icd = contents.join("Resources/vulkan/icd.d/MoltenVK_icd.json");
    Some(BundleRuntimePaths {
        gstreamer_plugins,
        gstreamer_scanner,
        gio_modules,
        libsoup,
        ca_bundle,
        vulkan_icd: icd.is_file().then_some(icd),
    })
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
    let registry_root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| {
            home.join("Library/Caches/io.github.lowband21.FerrexPlayer")
        })
        .unwrap_or_else(|| {
            std::env::temp_dir().join("io.github.lowband21.FerrexPlayer")
        });
    if std::fs::create_dir_all(&registry_root).is_err() {
        return;
    }
    let registry = registry_root.join("gstreamer-registry-1.0.bin");

    // SAFETY: `main` calls this as its first operation, before application,
    // logging, GStreamer, or mpv threads exist. No concurrent environment
    // access can have been initiated by Ferrex at this point.
    unsafe {
        std::env::set_var(
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            &paths.gstreamer_plugins,
        );
        std::env::set_var("GST_PLUGIN_PATH_1_0", &paths.gstreamer_plugins);
        std::env::set_var("GST_PLUGIN_SCANNER_1_0", &paths.gstreamer_scanner);
        std::env::set_var("GST_PLUGIN_SCANNER", &paths.gstreamer_scanner);
        std::env::set_var("GST_REGISTRY_1_0", registry);
        std::env::set_var("GIO_EXTRA_MODULES", &paths.gio_modules);
        if let Some(icd) = paths.vulkan_icd {
            std::env::set_var("VK_ICD_FILENAMES", icd);
        }
    }
    configure_bundle_tls(&paths.ca_bundle);
    preload_bundled_libsoup(&paths.libsoup);
}

#[cfg(target_os = "macos")]
fn configure_bundle_tls(path: &Path) {
    static DATABASE: OnceLock<usize> = OnceLock::new();
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return;
    };
    // SAFETY: GIO is linked into the macOS player; the path is a live,
    // NUL-terminated bundle resource. The database reference is intentionally
    // retained for process lifetime after becoming the backend default.
    let database =
        unsafe { g_tls_file_database_new(path.as_ptr(), std::ptr::null_mut()) };
    let backend = unsafe { g_tls_backend_get_default() };
    if database.is_null() || backend.is_null() {
        eprintln!(
            "bundled CA database initialization failed; bundled GStreamer HTTPS runtime is unavailable"
        );
        return;
    }
    unsafe { g_tls_backend_set_default_database(backend, database) };
    let _ = DATABASE.set(database as usize);
}

#[cfg(target_os = "macos")]
fn preload_bundled_libsoup(path: &Path) {
    const RTLD_LAZY: c_int = 0x1;
    const RTLD_GLOBAL: c_int = 0x8;
    static HANDLE: OnceLock<usize> = OnceLock::new();

    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return;
    };
    // SAFETY: the path points at the closure-audited bundled dylib, the C
    // string is NUL-terminated, and the returned handle is intentionally kept
    // alive for the process so GStreamer's later bare-leaf dlopen coalesces it.
    let handle = unsafe { dlopen(path.as_ptr(), RTLD_LAZY | RTLD_GLOBAL) };
    if !handle.is_null() {
        let _ = HANDLE.set(handle as usize);
    } else {
        eprintln!(
            "bundled libsoup preload failed; bundled GStreamer network runtime is unavailable"
        );
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
}

#[cfg(target_os = "macos")]
#[link(name = "gio-2.0")]
unsafe extern "C" {
    fn g_tls_file_database_new(
        anchors: *const c_char,
        error: *mut *mut c_void,
    ) -> *mut c_void;
    fn g_tls_backend_get_default() -> *mut c_void;
    fn g_tls_backend_set_default_database(
        backend: *mut c_void,
        database: *mut c_void,
    );
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

        let plugins = contents.join("Resources/gstreamer-1.0");
        let scanner = contents.join("Helpers/gst-plugin-scanner");
        let gio_modules = contents.join("Resources/gio/modules");
        let libsoup = contents.join("Frameworks/libsoup-3.0.0.dylib");
        let ca_bundle = contents.join("Resources/tls/cacert.pem");
        let icd = contents.join("Resources/vulkan/icd.d/MoltenVK_icd.json");
        std::fs::create_dir_all(&plugins).expect("create plugins");
        std::fs::create_dir_all(scanner.parent().expect("scanner parent"))
            .expect("create helpers");
        std::fs::write(&scanner, []).expect("create scanner");
        std::fs::create_dir_all(&gio_modules).expect("create GIO modules");
        std::fs::create_dir_all(libsoup.parent().expect("Frameworks parent"))
            .expect("create Frameworks");
        std::fs::write(&libsoup, []).expect("create libsoup");
        std::fs::create_dir_all(ca_bundle.parent().expect("CA parent"))
            .expect("create CA directory");
        std::fs::write(&ca_bundle, b"certificate").expect("create CA bundle");
        std::fs::create_dir_all(icd.parent().expect("ICD parent"))
            .expect("create ICD dir");
        std::fs::write(&icd, b"{}").expect("create ICD");

        assert_eq!(
            bundle_runtime_paths(&executable),
            Some(BundleRuntimePaths {
                gstreamer_plugins: plugins,
                gstreamer_scanner: scanner,
                gio_modules,
                libsoup,
                ca_bundle,
                vulkan_icd: Some(icd),
            })
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
