fn main() {
    // Release profiles can intentionally omit platform VOs for licensing.
    // Rebuild the capability preflight whenever that package contract changes.
    println!("cargo:rerun-if-env-changed=FERREX_MPV_X11");
    println!("cargo:rerun-if-env-changed=FERREX_MPV_WINDOWS_PRESENTER");
    println!("cargo:rerun-if-env-changed=FERREX_MPV_MACOS_PRESENTER");
}
