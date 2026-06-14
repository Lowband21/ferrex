use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::Digest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyDigest([u8; 16]);

impl KeyDigest {
    pub fn from_key_str(key: &str) -> Self {
        let digest = sha2::Sha256::digest(key.as_bytes());
        let mut out = [0u8; 16];
        out.copy_from_slice(&digest[..16]);
        Self(out)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AccessMeta {
    pub created_ms: u64,
    pub last_access_ms: u64,
}

#[derive(Debug)]
pub struct AccessIndex {
    path: PathBuf,
    entries: HashMap<KeyDigest, AccessMeta>,
    dirty: bool,
    last_flushed_ms: u64,
}

impl AccessIndex {
    const MAGIC: [u8; 8] = *b"FRXIMG01";
    const VERSION: u32 = 1;

    pub fn load_or_default(path: PathBuf, now_ms: u64) -> Self {
        let entries = load_file(&path).unwrap_or_default();
        Self {
            path,
            entries,
            dirty: false,
            last_flushed_ms: now_ms,
        }
    }

    pub fn touch(&mut self, digest: KeyDigest, now_ms: u64) {
        self.entries
            .entry(digest)
            .and_modify(|m| m.last_access_ms = now_ms)
            .or_insert_with(|| AccessMeta {
                created_ms: now_ms,
                last_access_ms: now_ms,
            });
        self.dirty = true;
    }

    pub fn insert_on_write(&mut self, digest: KeyDigest, now_ms: u64) {
        self.entries.entry(digest).or_insert_with(|| AccessMeta {
            created_ms: now_ms,
            last_access_ms: now_ms,
        });
        self.entries.entry(digest).and_modify(|m| {
            m.last_access_ms = now_ms;
        });
        self.dirty = true;
    }

    pub fn last_access_ms(&self, digest: &KeyDigest) -> Option<u64> {
        self.entries.get(digest).map(|m| m.last_access_ms)
    }

    pub fn remove(&mut self, digest: &KeyDigest) -> bool {
        let removed = self.entries.remove(digest).is_some();
        if removed {
            self.dirty = true;
        }
        removed
    }

    pub fn prune_not_in_set(&mut self, present: &HashSet<KeyDigest>) -> usize {
        let before = self.entries.len();
        self.entries.retain(|k, _| present.contains(k));
        let removed = before.saturating_sub(self.entries.len());
        if removed > 0 {
            self.dirty = true;
        }
        removed
    }

    pub fn should_flush(
        &self,
        now_ms: u64,
        flush_interval: std::time::Duration,
        under_pressure: bool,
    ) -> bool {
        if !self.dirty {
            return false;
        }
        if under_pressure {
            return true;
        }
        let interval_ms =
            flush_interval.as_millis().min(u128::from(u64::MAX)) as u64;
        now_ms.saturating_sub(self.last_flushed_ms) >= interval_ms.max(1)
    }

    pub fn prepare_flush(&mut self, now_ms: u64) -> Option<(PathBuf, Vec<u8>)> {
        if !self.dirty {
            return None;
        }
        let bytes = self.serialize();
        self.dirty = false;
        self.last_flushed_ms = now_ms;
        Some((self.path.clone(), bytes))
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + (self.entries.len() * 32));
        out.extend_from_slice(&Self::MAGIC);
        out.extend_from_slice(&Self::VERSION.to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for (digest, meta) in self.entries.iter() {
            out.extend_from_slice(&digest.0);
            out.extend_from_slice(&meta.created_ms.to_le_bytes());
            out.extend_from_slice(&meta.last_access_ms.to_le_bytes());
        }
        out
    }
}

fn load_file(path: &Path) -> anyhow::Result<HashMap<KeyDigest, AccessMeta>> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HashMap::new());
        }
        Err(e) => return Err(e.into()),
    };

    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    if buf.len() < 8 + 4 + 4 {
        return Ok(HashMap::new());
    }

    let mut cursor = 0usize;
    let magic: [u8; 8] = buf[cursor..cursor + 8].try_into()?;
    cursor += 8;
    if magic != AccessIndex::MAGIC {
        return Ok(HashMap::new());
    }

    let version = u32::from_le_bytes(buf[cursor..cursor + 4].try_into()?);
    cursor += 4;
    if version != AccessIndex::VERSION {
        return Ok(HashMap::new());
    }

    let count = u32::from_le_bytes(buf[cursor..cursor + 4].try_into()?);
    cursor += 4;

    let mut out = HashMap::with_capacity(count as usize);
    for _ in 0..count {
        if cursor + 16 + 8 + 8 > buf.len() {
            break;
        }
        let digest: [u8; 16] = buf[cursor..cursor + 16].try_into()?;
        cursor += 16;
        let created_ms =
            u64::from_le_bytes(buf[cursor..cursor + 8].try_into()?);
        cursor += 8;
        let last_access_ms =
            u64::from_le_bytes(buf[cursor..cursor + 8].try_into()?);
        cursor += 8;

        out.insert(
            KeyDigest(digest),
            AccessMeta {
                created_ms,
                last_access_ms,
            },
        );
    }
    Ok(out)
}

pub fn write_snapshot_sync(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("index path has no parent"))?;
    std::fs::create_dir_all(parent)?;

    let tmp_path = path.with_extension("tmp");
    {
        let mut f = File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(tmp_path, path)?;
    Ok(())
}
