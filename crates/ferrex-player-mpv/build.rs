//! Validate and locate the optional linked libmpv development library.

use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=LIBMPV_LIB_DIR");
    println!("cargo:rerun-if-env-changed=LIBMPV_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=LIBMPV_DLL_DIR");
    println!("cargo:rerun-if-env-changed=LIBMPV_DLL");
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    if env::var_os("CARGO_FEATURE_LINKED").is_none()
        || env::var_os("DOCS_RS").is_some()
    {
        return;
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS")
        .expect("Cargo must provide CARGO_CFG_TARGET_OS");

    if target_os == "windows" {
        let target_env = env::var("CARGO_CFG_TARGET_ENV")
            .expect("Cargo must provide CARGO_CFG_TARGET_ENV");
        let lib_dir =
            required_directory("LIBMPV_LIB_DIR", "libmpv import library");
        let include_dir =
            required_directory("LIBMPV_INCLUDE_DIR", "libmpv client headers");
        let client_header = include_dir.join("mpv").join("client.h");
        if !client_header.is_file() {
            panic!(
                "LIBMPV_INCLUDE_DIR must contain mpv/client.h; missing {}",
                client_header.display()
            )
        }

        let import_names: &[&str] = if target_env == "msvc" {
            &["mpv.lib"]
        } else {
            &["libmpv.dll.a", "mpv.dll.a"]
        };
        if find_first(&lib_dir, import_names).is_none() {
            panic!(
                "LIBMPV_LIB_DIR does not contain the required {target_env} \
                 import library (expected one of {} in {})",
                import_names.join(", "),
                lib_dir.display(),
            );
        }

        let runtime = env::var_os("LIBMPV_DLL")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("LIBMPV_DLL_DIR").and_then(|directory| {
                    find_first(
                        &PathBuf::from(directory),
                        &["libmpv-2.dll", "mpv-2.dll", "mpv.dll"],
                    )
                })
            })
            .unwrap_or_else(|| {
                panic!(
                    "the Windows `linked` feature requires the matching \
                     libmpv runtime; set LIBMPV_DLL to the DLL path or \
                     LIBMPV_DLL_DIR to its directory"
                )
            });
        if !runtime.is_file() {
            panic!("libmpv runtime DLL is missing: {}", runtime.display());
        }

        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        println!("cargo:rustc-env=FERREX_LIBMPV_DLL={}", runtime.display());
        return;
    }

    if let Err(error) = pkg_config::Config::new()
        .atleast_version("2.2.0")
        .probe("mpv")
    {
        panic!(
            "the `linked` feature requires libmpv client API 2.2 \
             (mpv >= 0.37) and its pkg-config metadata; install the development \
             package, enter `nix develop .#ferrex-player`, or disable the \
             higher-level `mpv` feature: {error}"
        );
    }
}

fn required_directory(variable: &str, purpose: &str) -> PathBuf {
    let directory = env::var_os(variable).map(PathBuf::from).unwrap_or_else(|| {
        panic!(
            "the Windows `linked` feature requires {purpose}; set {variable} \
             to its directory, or disable the higher-level `mpv` feature"
        )
    });
    if !directory.is_dir() {
        panic!(
            "{variable} does not name a directory: {}",
            directory.display()
        );
    }
    directory
}

fn find_first(directory: &std::path::Path, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file())
}
