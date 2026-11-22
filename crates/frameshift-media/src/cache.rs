use crate::frame::Rgba8Frame;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Asset identifier matching `AssetDef.id`.
pub type AssetId = String;

/// Cache key combining an asset ID and a frame index or quantized `Flicks` timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameCacheKey {
    pub asset_id: AssetId,
    pub frame_key: u64,
}

impl FrameCacheKey {
    /// Creates a new cache key.
    pub fn new(asset_id: impl Into<AssetId>, frame_key: u64) -> Self {
        Self {
            asset_id: asset_id.into(),
            frame_key,
        }
    }
}

impl From<(AssetId, u64)> for FrameCacheKey {
    fn from((asset_id, frame_key): (AssetId, u64)) -> Self {
        Self {
            asset_id,
            frame_key,
        }
    }
}

/// Telemetry counters reported by `LruFrameCache`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheTelemetry {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub current_entries: usize,
    pub capacity: usize,
    pub current_bytes: usize,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    frame: Arc<Rgba8Frame>,
    byte_size: usize,
    last_used_seq: u64,
}

/// Deterministic bounded-capacity LRU frame cache keyed by `(AssetId, u64)`.
#[derive(Debug)]
pub struct LruFrameCache {
    capacity: usize,
    max_bytes: Option<usize>,
    current_bytes: usize,
    access_seq: u64,
    entries: HashMap<FrameCacheKey, CacheEntry>,
    order: BTreeMap<u64, FrameCacheKey>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl LruFrameCache {
    /// Creates a new LRU frame cache holding at most `capacity` frames.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            max_bytes: None,
            current_bytes: 0,
            access_seq: 0,
            entries: HashMap::with_capacity(capacity),
            order: BTreeMap::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    /// Creates a new LRU frame cache bounded by both `capacity` frames and `max_bytes` pixel bytes.
    pub fn with_max_bytes(capacity: usize, max_bytes: usize) -> Self {
        let mut cache = Self::new(capacity);
        cache.max_bytes = Some(max_bytes);
        cache
    }

    /// Looks up a frame by `(asset_id, frame_key)` and updates LRU recency + hit/miss telemetry.
    pub fn get(&mut self, asset_id: &str, frame_key: u64) -> Option<Arc<Rgba8Frame>> {
        let key = FrameCacheKey::new(asset_id, frame_key);
        let Some(entry) = self.entries.get_mut(&key) else {
            self.misses += 1;
            return None;
        };
        let old_seq = entry.last_used_seq;
        self.order.remove(&old_seq);
        self.access_seq += 1;
        let new_seq = self.access_seq;
        entry.last_used_seq = new_seq;
        self.order.insert(new_seq, key);
        self.hits += 1;
        Some(Arc::clone(&entry.frame))
    }

    /// Inspects whether `(asset_id, frame_key)` is cached without mutating LRU order or telemetry.
    pub fn peek(&self, asset_id: &str, frame_key: u64) -> Option<Arc<Rgba8Frame>> {
        let key = FrameCacheKey::new(asset_id, frame_key);
        self.entries.get(&key).map(|e| Arc::clone(&e.frame))
    }

    /// Inserts or updates a cached frame, evicting least-recently-used entries deterministically
    /// until capacity and byte bounds are satisfied.
    pub fn insert(&mut self, asset_id: impl Into<AssetId>, frame_key: u64, frame: Rgba8Frame) {
        self.insert_arc(asset_id, frame_key, Arc::new(frame));
    }

    /// Inserts or updates a shared `Arc<Rgba8Frame>` entry.
    pub fn insert_arc(
        &mut self,
        asset_id: impl Into<AssetId>,
        frame_key: u64,
        frame: Arc<Rgba8Frame>,
    ) {
        if self.capacity == 0 {
            return;
        }
        let byte_size = frame.data.len();
        if let Some(max_bytes) = self.max_bytes
            && byte_size > max_bytes
        {
            return;
        }

        let key = FrameCacheKey::new(asset_id, frame_key);
        if let Some(existing) = self.entries.remove(&key) {
            self.order.remove(&existing.last_used_seq);
            self.current_bytes -= existing.byte_size;
        }

        while self.entries.len() >= self.capacity
            || self
                .max_bytes
                .is_some_and(|limit| self.current_bytes + byte_size > limit)
        {
            if !self.evict_oldest() {
                break;
            }
        }

        self.access_seq += 1;
        let seq = self.access_seq;
        self.current_bytes += byte_size;
        self.order.insert(seq, key.clone());
        self.entries.insert(
            key,
            CacheEntry {
                frame,
                byte_size,
                last_used_seq: seq,
            },
        );
    }

    /// Records a cache miss explicitly when callers use `get` + fallback decode.
    #[inline]
    pub fn record_miss(&mut self) {
        self.misses += 1;
    }

    /// Removes all cached frames belonging to `asset_id`.
    pub fn invalidate_asset(&mut self, asset_id: &str) {
        let keys_to_remove: Vec<FrameCacheKey> = self
            .entries
            .keys()
            .filter(|k| k.asset_id == asset_id)
            .cloned()
            .collect();
        for key in keys_to_remove {
            if let Some(entry) = self.entries.remove(&key) {
                self.order.remove(&entry.last_used_seq);
                self.current_bytes -= entry.byte_size;
            }
        }
    }

    /// Clears all entries while preserving cumulative telemetry or resetting everything.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.current_bytes = 0;
    }

    /// Returns current hit/miss/eviction telemetry and occupancy.
    pub fn telemetry(&self) -> CacheTelemetry {
        CacheTelemetry {
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            current_entries: self.entries.len(),
            capacity: self.capacity,
            current_bytes: self.current_bytes,
        }
    }

    /// Number of currently cached frames.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no frames are cached.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn evict_oldest(&mut self) -> bool {
        let Some((&oldest_seq, oldest_key)) = self.order.iter().next() else {
            return false;
        };
        let oldest_key = oldest_key.clone();
        self.order.remove(&oldest_seq);
        if let Some(removed) = self.entries.remove(&oldest_key) {
            self.current_bytes -= removed.byte_size;
            self.evictions += 1;
            true
        } else {
            false
        }
    }
}
