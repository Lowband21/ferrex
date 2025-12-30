/// Shared scanner defaults that align with server configuration knobs.
///
/// Keeping the extension list in one place allows the server to expose a user
/// facing configuration later without diverging from the core's filtering
/// rules.
pub const DEFAULT_VIDEO_FILE_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "mpg", "mpeg",
];

/// Convenience helper for consumers that work with owned strings (e.g. config
/// deserialisation layers).
pub fn default_video_file_extensions_vec() -> Vec<String> {
    DEFAULT_VIDEO_FILE_EXTENSIONS
        .iter()
        .map(|ext| ext.to_string())
        .collect()
}
