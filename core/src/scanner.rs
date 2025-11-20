use std::path::Path;
use walkdir::{DirEntry, WalkDir};
use crate::{MediaFile, MediaError, Result};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct MediaScanner {
    /// Supported video file extensions
    pub video_extensions: Vec<String>,
    /// Maximum depth for directory traversal (None = unlimited)
    pub max_depth: Option<usize>,
    /// Whether to follow symbolic links
    pub follow_links: bool,
}

impl Default for MediaScanner {
    fn default() -> Self {
        Self {
            video_extensions: vec![
                "mp4".to_string(),
                "mkv".to_string(),
                "avi".to_string(),
                "mov".to_string(),
                "webm".to_string(),
                "flv".to_string(),
                "wmv".to_string(),
                "m4v".to_string(),
                "mpg".to_string(),
                "mpeg".to_string(),
                "3gp".to_string(),
                "ogv".to_string(),
                "ts".to_string(),
                "mts".to_string(),
                "m2ts".to_string(),
            ],
            max_depth: None,
            follow_links: false,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanResult {
    pub total_files: usize,
    pub video_files: Vec<MediaFile>,
    pub skipped_files: usize,
    pub errors: Vec<String>,
}

impl MediaScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum directory depth for scanning
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Enable following symbolic links
    pub fn with_follow_links(mut self, follow: bool) -> Self {
        self.follow_links = follow;
        self
    }

    /// Add custom video extensions
    pub fn with_extensions(mut self, extensions: Vec<String>) -> Self {
        self.video_extensions = extensions;
        self
    }

    /// Check if a file is a supported video file based on extension
    pub fn is_video_file(&self, path: &Path) -> bool {
        if let Some(extension) = path.extension() {
            if let Some(ext_str) = extension.to_str() {
                let ext_lower = ext_str.to_lowercase();
                return self.video_extensions.contains(&ext_lower);
            }
        }
        false
    }

    /// Scan a directory for media files
    pub fn scan_directory<P: AsRef<Path>>(&self, root_path: P) -> Result<ScanResult> {
        let root_path = root_path.as_ref();
        
        info!("Starting media scan of: {} (follow_links: {})", root_path.display(), self.follow_links);
        
        if !root_path.exists() {
            return Err(MediaError::NotFound(format!(
                "Directory does not exist: {}",
                root_path.display()
            )));
        }

        if !root_path.is_dir() {
            return Err(MediaError::InvalidMedia(format!(
                "Path is not a directory: {}",
                root_path.display()
            )));
        }

        let mut walker = WalkDir::new(root_path)
            .follow_links(self.follow_links);
        
        if let Some(depth) = self.max_depth {
            walker = walker.max_depth(depth);
        }

        let mut result = ScanResult {
            total_files: 0,
            video_files: Vec::new(),
            skipped_files: 0,
            errors: Vec::new(),
        };

        for entry in walker {
            match entry {
                Ok(entry) => {
                    // Debug output for each entry
                    let path = entry.path();
                    let is_symlink = entry.path_is_symlink();
                    let file_type = entry.file_type();
                    
                    if is_symlink {
                        debug!("Walker found symlink: {} (is_dir: {})", path.display(), file_type.is_dir());
                    }
                    
                    if let Err(e) = self.process_entry(&entry, &mut result) {
                        warn!("Error processing {}: {}", entry.path().display(), e);
                        result.errors.push(format!("{}: {}", entry.path().display(), e));
                    }
                }
                Err(e) => {
                    warn!("Error walking directory: {}", e);
                    result.errors.push(format!("Directory walk error: {}", e));
                }
            }
        }

        info!(
            "Scan complete: {} total files, {} video files, {} skipped, {} errors",
            result.total_files,
            result.video_files.len(),
            result.skipped_files,
            result.errors.len()
        );

        Ok(result)
    }

    /// Process a single directory entry
    fn process_entry(&self, entry: &DirEntry, result: &mut ScanResult) -> Result<()> {
        // Log symlinks for debugging
        if entry.path_is_symlink() {
            if let Ok(target) = std::fs::read_link(entry.path()) {
                debug!("Found symlink: {} -> {}", entry.path().display(), target.display());
            }
        }
        
        // Skip directories
        if entry.file_type().is_dir() {
            return Ok(());
        }

        result.total_files += 1;
        let path = entry.path();

        debug!("Processing file: {}", path.display());

        // Check if it's a video file
        if !self.is_video_file(path) {
            result.skipped_files += 1;
            return Ok(());
        }

        // Create MediaFile from the path
        match MediaFile::new(path.to_path_buf()) {
            Ok(media_file) => {
                debug!("Found video file: {} ({})", media_file.filename, media_file.size);
                result.video_files.push(media_file);
            }
            Err(e) => {
                warn!("Failed to create MediaFile for {}: {}", path.display(), e);
                result.errors.push(format!("MediaFile creation failed: {}", e));
            }
        }

        Ok(())
    }

    /// Scan a single file
    pub fn scan_file<P: AsRef<Path>>(&self, file_path: P) -> Result<Option<MediaFile>> {
        let file_path = file_path.as_ref();
        
        if !file_path.exists() {
            return Err(MediaError::NotFound(format!(
                "File does not exist: {}",
                file_path.display()
            )));
        }

        if !file_path.is_file() {
            return Err(MediaError::InvalidMedia(format!(
                "Path is not a file: {}",
                file_path.display()
            )));
        }

        if !self.is_video_file(file_path) {
            return Ok(None);
        }

        MediaFile::new(file_path.to_path_buf()).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_video_file() {
        let scanner = MediaScanner::new();
        
        assert!(scanner.is_video_file(Path::new("test.mp4")));
        assert!(scanner.is_video_file(Path::new("TEST.MKV")));
        assert!(scanner.is_video_file(Path::new("movie.avi")));
        assert!(!scanner.is_video_file(Path::new("image.jpg")));
        assert!(!scanner.is_video_file(Path::new("document.txt")));
        assert!(!scanner.is_video_file(Path::new("no_extension")));
    }

    #[test]
    fn test_custom_extensions() {
        let scanner = MediaScanner::new()
            .with_extensions(vec!["test".to_string(), "custom".to_string()]);
        
        assert!(scanner.is_video_file(Path::new("file.test")));
        assert!(scanner.is_video_file(Path::new("file.custom")));
        assert!(!scanner.is_video_file(Path::new("file.mp4")));
    }

    #[test]
    fn test_scan_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let scanner = MediaScanner::new();
        
        let result = scanner.scan_directory(temp_dir.path()).unwrap();
        
        assert_eq!(result.total_files, 0);
        assert_eq!(result.video_files.len(), 0);
        assert_eq!(result.skipped_files, 0);
    }

    #[test]
    fn test_scan_nonexistent_directory() {
        let scanner = MediaScanner::new();
        let result = scanner.scan_directory("/nonexistent/path");
        
        assert!(result.is_err());
        if let Err(MediaError::NotFound(_)) = result {
            // Expected
        } else {
            panic!("Expected NotFound error");
        }
    }

    #[test]
    fn test_scan_with_mock_files() {
        let temp_dir = TempDir::new().unwrap();
        let scanner = MediaScanner::new();
        
        // Create test files
        fs::write(temp_dir.path().join("video.mp4"), b"fake video content").unwrap();
        fs::write(temp_dir.path().join("image.jpg"), b"fake image content").unwrap();
        fs::write(temp_dir.path().join("movie.mkv"), b"fake movie content").unwrap();
        
        let result = scanner.scan_directory(temp_dir.path()).unwrap();
        
        assert_eq!(result.total_files, 3);
        assert_eq!(result.video_files.len(), 2);
        assert_eq!(result.skipped_files, 1);
        
        // Check that we found the right files
        let filenames: Vec<_> = result.video_files.iter()
            .map(|f| f.filename.as_str())
            .collect();
        assert!(filenames.contains(&"video.mp4"));
        assert!(filenames.contains(&"movie.mkv"));
    }
}