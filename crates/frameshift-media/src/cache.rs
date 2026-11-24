use crate::frame::{DecodedVideoFrame, Nv12Frame, Rgba8Frame};
use frameshift_time::Flicks;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Default byte budget for [`LruFrameCache`] (`256 MiB`).
pub const DEFAULT_CACHE_MAX_BYTES: usize = 256 * 1024 * 1024;

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

/// Shared cached video frame payload (`Rgba8` or `Nv12`).
#[derive(Debug, Clone, PartialEq)]
pub enum CachedVideoFrame {
    Rgba8(Arc<Rgba8Frame>),
    Nv12(Arc<Nv12Frame>),
}

impl CachedVideoFrame {
    #[inline]
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Rgba8(f) => f.data.len(),
            Self::Nv12(f) => f.byte_len(),
        }
    }

    #[inline]
    pub fn width(&self) -> u32 {
        match self {
            Self::Rgba8(f) => f.width,
            Self::Nv12(f) => f.width,
        }
    }

    #[inline]
    pub fn height(&self) -> u32 {
        match self {
            Self::Rgba8(f) => f.height,
            Self::Nv12(f) => f.height,
        }
    }

    #[inline]
    pub fn pts(&self) -> Flicks {
        match self {
            Self::Rgba8(f) => f.pts,
            Self::Nv12(f) => f.pts,
        }
    }

    #[inline]
    pub fn as_rgba8(&self) -> Option<&Arc<Rgba8Frame>> {
        match self {
            Self::Rgba8(f) => Some(f),
            Self::Nv12(_) => None,
        }
    }

    #[inline]
    pub fn as_nv12(&self) -> Option<&Arc<Nv12Frame>> {
        match self {
            Self::Nv12(f) => Some(f),
            Self::Rgba8(_) => None,
        }
    }
}

impl From<DecodedVideoFrame> for CachedVideoFrame {
    fn from(frame: DecodedVideoFrame) -> Self {
        match frame {
            DecodedVideoFrame::Rgba8(f) => Self::Rgba8(Arc::new(f)),
            DecodedVideoFrame::Nv12(f) => Self::Nv12(Arc::new(f)),
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
    frame: CachedVideoFrame,
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
    /// Creates a new LRU frame cache holding at most `capacity` frames and bounded
    /// by [`DEFAULT_CACHE_MAX_BYTES`] (`256 MiB`).
    pub fn new(capacity: usize) -> Self {
        Self::with_max_bytes(capacity, DEFAULT_CACHE_MAX_BYTES)
    }

    /// Creates a new LRU frame cache bounded by both `capacity` frames and `max_bytes` pixel bytes.
    pub fn with_max_bytes(capacity: usize, max_bytes: usize) -> Self {
        Self {
            capacity,
            max_bytes: Some(max_bytes),
            current_bytes: 0,
            access_seq: 0,
            entries: HashMap::with_capacity(capacity),
            order: BTreeMap::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    /// Returns the configured byte budget ceiling, if any.
    #[inline]
    pub fn max_bytes(&self) -> Option<usize> {
        self.max_bytes
    }

    /// Looks up an `Rgba8Frame` by `(asset_id, frame_key)` and updates LRU recency + telemetry.
    pub fn get(&mut self, asset_id: &str, frame_key: u64) -> Option<Arc<Rgba8Frame>> {
        let key = FrameCacheKey::new(asset_id, frame_key);
        let Some(entry) = self.entries.get_mut(&key) else {
            self.misses += 1;
            return None;
        };
        let CachedVideoFrame::Rgba8(ref rgba) = entry.frame else {
            self.misses += 1;
            return None;
        };
        let rgba = Arc::clone(rgba);
        let old_seq = entry.last_used_seq;
        self.order.remove(&old_seq);
        self.access_seq += 1;
        let new_seq = self.access_seq;
        entry.last_used_seq = new_seq;
        self.order.insert(new_seq, key);
        self.hits += 1;
        Some(rgba)
    }

    /// Looks up either an `Rgba8` or `Nv12` frame by `(asset_id, frame_key)` and updates LRU recency.
    pub fn get_decoded(&mut self, asset_id: &str, frame_key: u64) -> Option<CachedVideoFrame> {
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
        Some(entry.frame.clone())
    }

    /// Inspects whether `(asset_id, frame_key)` is cached as `Rgba8Frame` without mutating LRU order.
    pub fn peek(&self, asset_id: &str, frame_key: u64) -> Option<Arc<Rgba8Frame>> {
        let key = FrameCacheKey::new(asset_id, frame_key);
        self.entries
            .get(&key)
            .and_then(|e| e.frame.as_rgba8().map(Arc::clone))
    }

    /// Inspects whether `(asset_id, frame_key)` is cached (`Rgba8` or `Nv12`) without mutating LRU order.
    pub fn peek_decoded(&self, asset_id: &str, frame_key: u64) -> Option<CachedVideoFrame> {
        let key = FrameCacheKey::new(asset_id, frame_key);
        self.entries.get(&key).map(|e| e.frame.clone())
    }

    /// Inserts or updates a cached `Rgba8Frame`, evicting least-recently-used entries deterministically
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
        self.insert_cached(asset_id, frame_key, CachedVideoFrame::Rgba8(frame));
    }

    /// Inserts or updates a cached `Nv12Frame`.
    pub fn insert_nv12(&mut self, asset_id: impl Into<AssetId>, frame_key: u64, frame: Nv12Frame) {
        self.insert_cached(asset_id, frame_key, CachedVideoFrame::Nv12(Arc::new(frame)));
    }

    /// Inserts or updates a cached [`DecodedVideoFrame`] (`Rgba8` or `Nv12`).
    pub fn insert_decoded(
        &mut self,
        asset_id: impl Into<AssetId>,
        frame_key: u64,
        frame: DecodedVideoFrame,
    ) {
        self.insert_cached(asset_id, frame_key, CachedVideoFrame::from(frame));
    }

    /// Inserts or updates a [`CachedVideoFrame`] (`Rgba8` or `Nv12`).
    pub fn insert_cached(
        &mut self,
        asset_id: impl Into<AssetId>,
        frame_key: u64,
        frame: CachedVideoFrame,
    ) {
        if self.capacity == 0 {
            return;
        }
        let byte_size = frame.byte_len();
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
