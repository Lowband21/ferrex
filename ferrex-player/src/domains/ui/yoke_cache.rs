use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use uuid::Uuid;

/// Minimal LRU-style cache for yokes keyed by media UUID
#[derive(Debug)]
pub struct YokeCache<Y> {
    cap: usize,
    inner: RwLock<Inner<Y>>,
    // Optional generation for invalidation (not wired in PoC)
    _generation: u64,
}

#[derive(Debug)]
struct Inner<Y> {
    map: HashMap<Uuid, Arc<Y>>,
    // front = most-recent, back = least-recent
    lru: VecDeque<Uuid>,
}

impl<Y> YokeCache<Y> {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            inner: RwLock::new(Inner {
                map: HashMap::new(),
                lru: VecDeque::new(),
            }),
            _generation: 0,
        }
    }

    pub fn clear(&self) {
        let mut inner = self.inner.write();
        inner.map.clear();
        inner.lru.clear();
    }

    pub fn len(&self) -> usize {
        self.inner.read().map.len()
    }

    pub fn contains_key(&self, id: &Uuid) -> bool {
        self.inner.read().map.contains_key(id)
    }

    /// Get a cloned Arc without moving LRU position
    pub fn peek_ref(&self, id: &Uuid) -> Option<Arc<Y>> {
        self.inner.read().map.get(id).cloned()
    }

    /// Get a cloned Arc without moving LRU position (alias)
    pub fn peek(&self, id: &Uuid) -> Option<Arc<Y>> {
        self.inner.read().map.get(id).cloned()
    }

    /// Get and bump LRU
    pub fn get(&self, id: &Uuid) -> Option<Arc<Y>> {
        let mut inner = self.inner.write();
        if inner.map.contains_key(id) {
            if let Some(pos) = inner.lru.iter().position(|x| x == id) {
                inner.lru.remove(pos);
            }
            inner.lru.push_front(*id);
            inner.map.get(id).cloned()
        } else {
            None
        }
    }

    /// Insert or update (LRU-aware)
    pub fn insert(&self, id: Uuid, yoke: Arc<Y>) {
        let mut inner = self.inner.write();
        if let std::collections::hash_map::Entry::Occupied(mut e) =
            inner.map.entry(id)
        {
            e.insert(yoke);
            if let Some(pos) = inner.lru.iter().position(|x| x == &id) {
                inner.lru.remove(pos);
            }
            inner.lru.push_front(id);
            return;
        }

        // Evict if at capacity
        if inner.map.len() >= self.cap
            && let Some(old) = inner.lru.pop_back()
        {
            inner.map.remove(&old);
        }

        inner.map.insert(id, yoke);
        inner.lru.push_front(id);
    }
}
