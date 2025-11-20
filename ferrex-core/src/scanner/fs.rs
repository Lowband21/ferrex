use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

/// Minimal, async-capable filesystem abstraction used by scanners.
#[async_trait]
pub trait FileSystem: Send + Sync {
    /// Check whether a path exists.
    async fn path_exists(&self, path: &Path) -> bool;

    /// Open a directory for iteration.
    async fn read_dir(&self, path: &Path) -> Result<Box<dyn ReadDirStream + Send>, String>;

    /// Fetch lightweight file metadata.
    async fn metadata(&self, path: &Path) -> Result<FsMetadata, String>;
}

/// Lightweight metadata needed by scanners.
#[derive(Debug, Clone, Copy)]
pub struct FsMetadata {
    pub is_dir: bool,
    pub is_file: bool,
    pub len: u64,
    /// Last modified time if available
    pub modified: Option<std::time::SystemTime>,
}

/// Async directory iterator (similar to tokio::fs::ReadDir).
#[async_trait]
pub trait ReadDirStream {
    /// Return next entry's path, or None when exhausted.
    async fn next_entry(&mut self) -> Result<Option<PathBuf>, String>;
}

/// Real filesystem implementation backed by tokio::fs.
pub struct RealFs;

impl RealFs {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl FileSystem for RealFs {
    async fn path_exists(&self, path: &Path) -> bool {
        // try_exists avoids errors for permission issues by returning false
        tokio::fs::try_exists(path).await.unwrap_or(false)
    }

    async fn read_dir(&self, path: &Path) -> Result<Box<dyn ReadDirStream + Send>, String> {
        let rd = tokio::fs::read_dir(path)
            .await
            .map_err(|e| format!("read_dir failed for {:?}: {}", path, e))?;
        Ok(Box::new(RealReadDir { inner: rd }))
    }

    async fn metadata(&self, path: &Path) -> Result<FsMetadata, String> {
        let md = tokio::fs::metadata(path)
            .await
            .map_err(|e| format!("metadata failed for {:?}: {}", path, e))?;
        Ok(FsMetadata {
            is_dir: md.is_dir(),
            is_file: md.is_file(),
            len: md.len(),
            modified: md.modified().ok(),
        })
    }
}

struct RealReadDir {
    inner: tokio::fs::ReadDir,
}

#[async_trait]
impl ReadDirStream for RealReadDir {
    async fn next_entry(&mut self) -> Result<Option<PathBuf>, String> {
        match self.inner.next_entry().await {
            Ok(Some(entry)) => Ok(Some(entry.path())),
            Ok(None) => Ok(None),
            Err(e) => Err(format!("next_entry failed: {}", e)),
        }
    }
}

/// In-memory filesystem for tests.
/// Note: Paths are treated literally; callers should use consistent absolute or relative paths.
#[derive(Default, Clone)]
pub struct InMemoryFs {
    nodes: HashMap<PathBuf, Node>,
}

#[derive(Clone)]
enum Node {
    Dir { children: Vec<PathBuf> },
    File { len: u64 },
}

impl InMemoryFs {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    pub fn add_dir<P: Into<PathBuf>>(&mut self, path: P) {
        let path = path.into();
        if self.nodes.contains_key(&path) {
            return;
        }
        self.ensure_parent_link(&path);
        self.nodes.insert(
            path,
            Node::Dir {
                children: Vec::new(),
            },
        );
    }

    pub fn add_file<P: Into<PathBuf>>(&mut self, path: P, len: u64) {
        let path = path.into();
        self.ensure_parent_link(&path);
        self.nodes.insert(path, Node::File { len });
    }

    fn ensure_parent_link(&mut self, path: &Path) {
        if let Some(parent) = path.parent() {
            // Ensure parent directory exists
            if !self.nodes.contains_key(parent) {
                self.nodes.insert(
                    parent.to_path_buf(),
                    Node::Dir {
                        children: Vec::new(),
                    },
                );
                // Recurse to ensure its parent exists
                self.ensure_parent_link(parent);
            }
            // Link child into parent
            if let Some(Node::Dir { children }) = self.nodes.get_mut(parent) {
                if !children.iter().any(|p| p.as_path() == path) {
                    children.push(path.to_path_buf());
                }
            }
        }
    }
}

#[async_trait]
impl FileSystem for InMemoryFs {
    async fn path_exists(&self, path: &Path) -> bool {
        self.nodes.contains_key(path)
    }

    async fn read_dir(&self, path: &Path) -> Result<Box<dyn ReadDirStream + Send>, String> {
        match self.nodes.get(path) {
            Some(Node::Dir { children }) => Ok(Box::new(InMemReadDir {
                queue: children.clone().into(),
            })),
            Some(Node::File { .. }) => Err(format!("read_dir on file: {:?}", path)),
            None => Err(format!("read_dir on missing path: {:?}", path)),
        }
    }

    async fn metadata(&self, path: &Path) -> Result<FsMetadata, String> {
        match self.nodes.get(path) {
            Some(Node::Dir { .. }) => Ok(FsMetadata {
                is_dir: true,
                is_file: false,
                len: 0,
                modified: None,
            }),
            Some(Node::File { len }) => Ok(FsMetadata {
                is_dir: false,
                is_file: true,
                len: *len,
                modified: None,
            }),
            None => Err(format!("metadata on missing path: {:?}", path)),
        }
    }
}

struct InMemReadDir {
    queue: VecDeque<PathBuf>,
}

#[async_trait]
impl ReadDirStream for InMemReadDir {
    async fn next_entry(&mut self) -> Result<Option<PathBuf>, String> {
        Ok(self.queue.pop_front())
    }
}
