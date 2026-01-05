//! Memory Budget Cache
//!
//! Provides LRU-based cache management with byte-level memory tracking
//! for partition eviction in the lazy graph manager.
//!
//! Thread-safe via interior mutability using parking_lot::Mutex.

use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;

/// Constants for memory estimation
const NODE_BASE_SIZE: usize = 512; // Base size of Node struct + typical string data
const EDGE_BASE_SIZE: usize = 128; // EdgeData + indices
const GRAPH_OVERHEAD: f64 = 1.4; // petgraph internal overhead factor

/// Default memory budget (512 MB)
const DEFAULT_MEMORY_BUDGET: usize = 512 * 1024 * 1024;

/// Minimum partitions to keep (avoid thrashing)
const DEFAULT_MIN_PARTITIONS: usize = 2;

/// Statistics about a loaded partition
#[derive(Debug, Clone)]
pub struct PartitionStats {
    /// Number of nodes in the partition
    pub node_count: usize,
    /// Number of edges in the partition
    pub edge_count: usize,
    /// Estimated memory footprint in bytes
    pub estimated_bytes: usize,
}

impl PartitionStats {
    /// Create stats for a partition
    pub fn new(node_count: usize, edge_count: usize) -> Self {
        let estimated_bytes = estimate_memory(node_count, edge_count);
        Self {
            node_count,
            edge_count,
            estimated_bytes,
        }
    }
}

/// Cache metrics for monitoring
#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    /// Number of cache hits (partition already loaded)
    pub hits: u64,
    /// Number of cache misses (partition needed loading)
    pub misses: u64,
    /// Number of partitions evicted
    pub evictions: u64,
    /// Total bytes evicted
    pub bytes_evicted: usize,
}

impl CacheMetrics {
    /// Get hit rate as a percentage (0.0 - 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Record a cache hit
    pub fn record_hit(&mut self) {
        self.hits += 1;
    }

    /// Record a cache miss
    pub fn record_miss(&mut self) {
        self.misses += 1;
    }

    /// Record an eviction
    pub fn record_eviction(&mut self, bytes: usize) {
        self.evictions += 1;
        self.bytes_evicted += bytes;
    }
}

/// Inner state for MemoryBudgetCache (protected by Mutex)
struct CacheState {
    /// Current estimated memory usage in bytes
    current_memory_bytes: usize,

    /// LRU cache mapping partition ID to its stats
    /// Most recently used partitions are at the "front"
    partition_lru: LruCache<String, PartitionStats>,

    /// Cache metrics
    metrics: CacheMetrics,
}

/// Memory budget cache with LRU eviction
///
/// Tracks loaded partitions and their memory footprint, evicting
/// least-recently-used partitions when the memory budget is exceeded.
///
/// Thread-safe: All methods take `&self` and use interior mutability
/// via parking_lot::Mutex for concurrent access.
pub struct MemoryBudgetCache {
    /// Maximum memory budget in bytes (immutable after construction)
    max_memory_bytes: usize,

    /// Minimum partitions to keep (immutable after construction)
    min_partitions: usize,

    /// Mutable state protected by Mutex
    state: Mutex<CacheState>,
}

impl MemoryBudgetCache {
    /// Create a new cache with the given memory budget
    pub fn new(max_memory_bytes: usize) -> Self {
        Self {
            max_memory_bytes,
            min_partitions: DEFAULT_MIN_PARTITIONS,
            state: Mutex::new(CacheState {
                current_memory_bytes: 0,
                // Use a large cap - we manage eviction ourselves based on bytes
                partition_lru: LruCache::new(NonZeroUsize::new(10000).unwrap()),
                metrics: CacheMetrics::default(),
            }),
        }
    }

    /// Create a cache with default memory budget (512 MB)
    pub fn with_default_budget() -> Self {
        Self::new(DEFAULT_MEMORY_BUDGET)
    }

    /// Set the minimum number of partitions to keep
    pub fn with_min_partitions(mut self, min: usize) -> Self {
        self.min_partitions = min;
        self
    }

    /// Get the memory budget in bytes
    pub fn max_memory_bytes(&self) -> usize {
        self.max_memory_bytes
    }

    /// Get current memory usage in bytes
    pub fn current_memory_bytes(&self) -> usize {
        self.state.lock().current_memory_bytes
    }

    /// Get memory usage as a percentage (0.0 - 1.0)
    pub fn memory_usage_ratio(&self) -> f64 {
        if self.max_memory_bytes == 0 {
            0.0
        } else {
            let current = self.state.lock().current_memory_bytes;
            current as f64 / self.max_memory_bytes as f64
        }
    }

    /// Get the number of partitions currently tracked
    pub fn partition_count(&self) -> usize {
        self.state.lock().partition_lru.len()
    }

    /// Get a snapshot of cache metrics
    pub fn metrics(&self) -> CacheMetrics {
        self.state.lock().metrics.clone()
    }

    /// Reset cache metrics
    pub fn reset_metrics(&self) {
        self.state.lock().metrics = CacheMetrics::default();
    }

    /// Check if a partition is in the cache
    pub fn contains(&self, partition_id: &str) -> bool {
        self.state.lock().partition_lru.contains(partition_id)
    }

    /// Mark a partition as accessed (updates LRU order)
    ///
    /// Returns true if the partition was found, false otherwise.
    pub fn touch(&self, partition_id: &str) -> bool {
        let mut state = self.state.lock();
        if state.partition_lru.get(partition_id).is_some() {
            state.metrics.record_hit();
            true
        } else {
            state.metrics.record_miss();
            false
        }
    }

    /// Record that a partition has been loaded
    ///
    /// Adds the partition to the cache and updates memory tracking.
    pub fn record_loaded(&self, partition_id: String, stats: PartitionStats) {
        let mut state = self.state.lock();
        state.current_memory_bytes += stats.estimated_bytes;
        state.partition_lru.put(partition_id, stats);
    }

    /// Get stats for a partition (if tracked)
    ///
    /// Returns a clone of the stats to avoid holding the lock.
    pub fn get_stats(&self, partition_id: &str) -> Option<PartitionStats> {
        self.state.lock().partition_lru.peek(partition_id).cloned()
    }

    /// Remove a partition from the cache
    ///
    /// Returns the stats if the partition was tracked, None otherwise.
    pub fn remove(&self, partition_id: &str) -> Option<PartitionStats> {
        let mut state = self.state.lock();
        if let Some(stats) = state.partition_lru.pop(partition_id) {
            state.current_memory_bytes = state
                .current_memory_bytes
                .saturating_sub(stats.estimated_bytes);
            state.metrics.record_eviction(stats.estimated_bytes);
            Some(stats)
        } else {
            None
        }
    }

    /// Check if memory budget is exceeded
    pub fn is_over_budget(&self) -> bool {
        self.state.lock().current_memory_bytes > self.max_memory_bytes
    }

    /// Get partition IDs that should be evicted to make room
    ///
    /// Returns partition IDs in LRU order (least recently used first).
    /// Respects `min_partitions` to avoid thrashing.
    pub fn get_eviction_candidates(&self) -> Vec<String> {
        let state = self.state.lock();
        if state.current_memory_bytes <= self.max_memory_bytes {
            return vec![];
        }

        let mut candidates = Vec::new();
        let mut projected_bytes = state.current_memory_bytes;
        let mut remaining_partitions = state.partition_lru.len();

        // LruCache::iter() returns MRU first, so .rev() gives us LRU first
        for (partition_id, stats) in state.partition_lru.iter().rev() {
            if projected_bytes <= self.max_memory_bytes {
                break;
            }
            if remaining_partitions <= self.min_partitions {
                break;
            }

            candidates.push(partition_id.clone());
            projected_bytes = projected_bytes.saturating_sub(stats.estimated_bytes);
            remaining_partitions -= 1;
        }

        candidates
    }

    /// Get the amount of memory needed to be freed to accommodate a new load
    pub fn memory_needed_for(&self, additional_bytes: usize) -> usize {
        let current = self.state.lock().current_memory_bytes;
        let projected = current + additional_bytes;
        projected.saturating_sub(self.max_memory_bytes)
    }

    /// Get partitions to evict to make room for a specific amount of memory
    ///
    /// Returns partition IDs in LRU order.
    pub fn get_eviction_candidates_for(&self, needed_bytes: usize) -> Vec<String> {
        if needed_bytes == 0 {
            return vec![];
        }

        let state = self.state.lock();
        let mut candidates = Vec::new();
        let mut bytes_freed = 0usize;
        let mut remaining_partitions = state.partition_lru.len();

        // LruCache::iter() returns MRU first, so .rev() gives us LRU first
        for (partition_id, stats) in state.partition_lru.iter().rev() {
            if bytes_freed >= needed_bytes {
                break;
            }
            if remaining_partitions <= self.min_partitions {
                break;
            }

            candidates.push(partition_id.clone());
            bytes_freed += stats.estimated_bytes;
            remaining_partitions -= 1;
        }

        candidates
    }

    /// Clear all tracked partitions
    pub fn clear(&self) {
        let mut state = self.state.lock();
        state.partition_lru.clear();
        state.current_memory_bytes = 0;
    }
}

impl Default for MemoryBudgetCache {
    fn default() -> Self {
        Self::with_default_budget()
    }
}

/// Estimate memory footprint for a partition
///
/// Uses conservative estimates for Node and EdgeData sizes.
pub fn estimate_memory(node_count: usize, edge_count: usize) -> usize {
    let base = node_count * NODE_BASE_SIZE + edge_count * EDGE_BASE_SIZE;
    (base as f64 * GRAPH_OVERHEAD) as usize
}

/// Estimate memory footprint with custom size factors
pub fn estimate_memory_custom(
    node_count: usize,
    edge_count: usize,
    node_size: usize,
    edge_size: usize,
    overhead: f64,
) -> usize {
    let base = node_count * node_size + edge_count * edge_size;
    (base as f64 * overhead) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_estimation() {
        // 1000 nodes, 500 edges
        let bytes = estimate_memory(1000, 500);
        // Expected: (1000 * 512 + 500 * 128) * 1.4 = (512000 + 64000) * 1.4 = 806400
        assert_eq!(bytes, 806400);
    }

    #[test]
    fn test_cache_new() {
        let cache = MemoryBudgetCache::new(1024 * 1024); // 1 MB
        assert_eq!(cache.max_memory_bytes(), 1024 * 1024);
        assert_eq!(cache.current_memory_bytes(), 0);
        assert_eq!(cache.partition_count(), 0);
    }

    #[test]
    fn test_cache_record_loaded() {
        let cache = MemoryBudgetCache::new(10_000_000); // 10 MB

        let stats = PartitionStats::new(100, 50);
        let expected_bytes = stats.estimated_bytes;

        cache.record_loaded("partition1".to_string(), stats);

        assert_eq!(cache.partition_count(), 1);
        assert_eq!(cache.current_memory_bytes(), expected_bytes);
        assert!(cache.contains("partition1"));
    }

    #[test]
    fn test_cache_touch() {
        let cache = MemoryBudgetCache::new(10_000_000);

        cache.record_loaded("p1".to_string(), PartitionStats::new(100, 50));
        cache.record_loaded("p2".to_string(), PartitionStats::new(100, 50));

        // Touch should update LRU order and record hit
        assert!(cache.touch("p1"));
        assert_eq!(cache.metrics().hits, 1);

        // Miss for non-existent partition
        assert!(!cache.touch("p3"));
        assert_eq!(cache.metrics().misses, 1);
    }

    #[test]
    fn test_cache_remove() {
        let cache = MemoryBudgetCache::new(10_000_000);

        let stats = PartitionStats::new(100, 50);
        let bytes = stats.estimated_bytes;

        cache.record_loaded("p1".to_string(), stats);
        assert_eq!(cache.current_memory_bytes(), bytes);

        let removed = cache.remove("p1");
        assert!(removed.is_some());
        assert_eq!(cache.current_memory_bytes(), 0);
        assert!(!cache.contains("p1"));
        assert_eq!(cache.metrics().evictions, 1);
    }

    #[test]
    fn test_cache_over_budget() {
        let cache = MemoryBudgetCache::new(100_000); // 100 KB budget

        // Add partition that's under budget
        cache.record_loaded("p1".to_string(), PartitionStats::new(50, 25)); // ~50KB
        assert!(!cache.is_over_budget());

        // Add partition that pushes over budget
        cache.record_loaded("p2".to_string(), PartitionStats::new(100, 50)); // ~100KB
        assert!(cache.is_over_budget());
    }

    #[test]
    fn test_cache_eviction_candidates() {
        // Each partition is ~24KB (30*512 + 15*128) * 1.4 = ~24192 bytes
        // Set budget to 50KB so 3 partitions (~72KB) will exceed it
        let cache = MemoryBudgetCache::new(50_000).with_min_partitions(1);

        // Add several partitions (in order: p1 oldest, p2, p3 newest)
        cache.record_loaded("p1".to_string(), PartitionStats::new(30, 15));
        cache.record_loaded("p2".to_string(), PartitionStats::new(30, 15));
        cache.record_loaded("p3".to_string(), PartitionStats::new(30, 15));

        // Should be over budget
        assert!(cache.is_over_budget());

        // Touch p1 to make it most recently used
        cache.touch("p1");

        // Now LRU order is: p2 (oldest/LRU), p3, p1 (newest/MRU)
        // Get candidates when over budget - should evict oldest first
        let candidates = cache.get_eviction_candidates();
        assert!(!candidates.is_empty());
        // p2 is the oldest (LRU) after touching p1
        assert_eq!(candidates[0], "p2");
    }

    #[test]
    fn test_cache_eviction_candidates_for() {
        let cache = MemoryBudgetCache::new(1_000_000).with_min_partitions(1);

        // Add partitions with known sizes
        let stats1 = PartitionStats::new(100, 50); // ~80KB
        let stats2 = PartitionStats::new(100, 50);

        cache.record_loaded("p1".to_string(), stats1.clone());
        cache.record_loaded("p2".to_string(), stats2.clone());

        // Need to free 50KB
        let candidates = cache.get_eviction_candidates_for(50_000);
        assert!(!candidates.is_empty());
    }

    #[test]
    fn test_cache_min_partitions() {
        let cache = MemoryBudgetCache::new(10_000).with_min_partitions(2);

        // Add 3 partitions that exceed budget
        cache.record_loaded("p1".to_string(), PartitionStats::new(50, 25));
        cache.record_loaded("p2".to_string(), PartitionStats::new(50, 25));
        cache.record_loaded("p3".to_string(), PartitionStats::new(50, 25));

        // Should be over budget
        assert!(cache.is_over_budget());

        // But should only suggest evicting down to min_partitions (2)
        let candidates = cache.get_eviction_candidates();
        assert_eq!(candidates.len(), 1); // Can only evict 1 (3 - 2 = 1)
    }

    #[test]
    fn test_cache_metrics() {
        let cache = MemoryBudgetCache::new(10_000_000);

        cache.record_loaded("p1".to_string(), PartitionStats::new(100, 50));

        // Record some hits and misses
        cache.touch("p1"); // hit
        cache.touch("p1"); // hit
        cache.touch("p2"); // miss

        assert_eq!(cache.metrics().hits, 2);
        assert_eq!(cache.metrics().misses, 1);
        assert!((cache.metrics().hit_rate() - 0.666).abs() < 0.01);

        // Remove and check eviction metrics
        cache.remove("p1");
        assert_eq!(cache.metrics().evictions, 1);
    }

    #[test]
    fn test_cache_clear() {
        let cache = MemoryBudgetCache::new(10_000_000);

        cache.record_loaded("p1".to_string(), PartitionStats::new(100, 50));
        cache.record_loaded("p2".to_string(), PartitionStats::new(100, 50));

        cache.clear();

        assert_eq!(cache.partition_count(), 0);
        assert_eq!(cache.current_memory_bytes(), 0);
    }

    #[test]
    fn test_memory_usage_ratio() {
        let cache = MemoryBudgetCache::new(1_000_000); // 1 MB

        assert_eq!(cache.memory_usage_ratio(), 0.0);

        // Add ~500KB worth
        cache.record_loaded("p1".to_string(), PartitionStats::new(500, 250));

        let ratio = cache.memory_usage_ratio();
        assert!(ratio > 0.0 && ratio < 1.0);
    }
}
