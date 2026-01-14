//! Checkpoint/resume support for streaming graph indexing.
//!
//! This module provides checkpoint functionality for large repository indexing operations.
//! When indexing is interrupted (e.g., Ctrl+C, system shutdown), the checkpoint file
//! preserves progress so indexing can resume from where it left off.
//!
//! # Checkpoint Granularity
//!
//! Checkpoints are saved at partition boundaries. If indexing is interrupted mid-partition,
//! that partition will be re-indexed from scratch on resume (Qdrant upserts are idempotent).
//!
//! # Usage
//!
//! ```rust,ignore
//! // Load existing checkpoint or create new one
//! let checkpoint = IndexCheckpoint::load_or_create(
//!     &checkpoint_path,
//!     repo_id,
//!     manifest_hash,
//!     qdrant_url,
//! )?;
//!
//! // Check if we should resume
//! if checkpoint.completed_partitions.contains(&partition_id) {
//!     continue; // Skip already-indexed partition
//! }
//!
//! // Mark partition as starting
//! checkpoint.start_partition(&partition_id);
//! checkpoint.save(&checkpoint_path)?;
//!
//! // ... index partition ...
//!
//! // Mark partition as completed
//! checkpoint.complete_partition(&partition_id, &stats);
//! checkpoint.save(&checkpoint_path)?;
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Current checkpoint schema version.
pub const CHECKPOINT_VERSION: &str = "1.0";

/// Errors that can occur during checkpoint operations.
#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid checkpoint: {0}")]
    Invalid(String),
}

/// State of an indexing operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    /// Indexing is in progress.
    InProgress,
    /// Indexing completed successfully.
    Completed,
    /// Indexing failed with an error.
    Failed { error: String },
}

/// Statistics tracked in checkpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckpointStats {
    /// Total nodes processed (seen).
    pub total_processed: usize,
    /// Total nodes successfully indexed.
    pub total_indexed: usize,
    /// Total nodes intentionally skipped (FILE nodes, empty content).
    pub total_skipped: usize,
    /// Total nodes that failed to index.
    pub total_failed: usize,
    /// Points indexed to semantic collection.
    pub semantic_indexed: usize,
    /// Points indexed to code collection.
    pub code_indexed: usize,
    /// Number of partitions processed.
    pub partitions_processed: usize,
}

impl CheckpointStats {
    /// Accumulate stats from an indexing operation.
    pub fn accumulate(&mut self, other: &crate::IndexStats) {
        self.total_processed += other.total_processed;
        self.total_indexed += other.total_indexed;
        self.total_skipped += other.total_skipped;
        self.total_failed += other.total_failed;
        self.semantic_indexed += other.semantic_indexed;
        self.code_indexed += other.code_indexed;
        self.partitions_processed += 1;
    }
}

/// Result of checkpoint resume validation.
#[derive(Debug)]
pub enum ResumeValidation {
    /// Can resume from checkpoint.
    Valid,
    /// Manifest changed - must start fresh.
    ManifestChanged { old_hash: String, new_hash: String },
    /// Different Qdrant URL - must start fresh.
    QdrantUrlMismatch { old_url: String, new_url: String },
    /// Checkpoint is for a different repo.
    RepoMismatch { old_repo: String, new_repo: String },
    /// Previous indexing completed - nothing to resume.
    AlreadyCompleted,
    /// Previous indexing failed - can optionally resume.
    PreviousFailed { error: String },
}

/// Checkpoint file structure for resumable indexing.
///
/// This struct is serialized to `.codeprysm/index_checkpoint.json` and tracks
/// which partitions have been successfully indexed, allowing interrupted indexing
/// to resume from where it left off.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexCheckpoint {
    /// Schema version for compatibility checking.
    pub version: String,

    /// Repository identifier.
    pub repo_id: String,

    /// SHA-256 hash of manifest.json content.
    /// Used to detect if the graph changed since checkpoint was created.
    pub manifest_hash: String,

    /// Qdrant URL used for this index operation.
    /// Prevents mixing data across different Qdrant instances.
    pub qdrant_url: String,

    /// When indexing started.
    pub started_at: DateTime<Utc>,

    /// When checkpoint was last updated.
    pub last_updated: DateTime<Utc>,

    /// Current state of indexing.
    pub state: IndexState,

    /// Set of partition IDs that completed successfully.
    /// Using HashSet for O(1) lookup during resume.
    pub completed_partitions: HashSet<String>,

    /// Partition currently being processed (if any).
    /// This partition will be re-indexed on resume (may be partial).
    pub current_partition: Option<String>,

    /// Accumulated statistics from completed partitions.
    pub stats: CheckpointStats,
}

impl IndexCheckpoint {
    /// Create a new checkpoint for starting an indexing operation.
    pub fn new(repo_id: String, manifest_hash: String, qdrant_url: String) -> Self {
        let now = Utc::now();
        Self {
            version: CHECKPOINT_VERSION.to_string(),
            repo_id,
            manifest_hash,
            qdrant_url,
            started_at: now,
            last_updated: now,
            state: IndexState::InProgress,
            completed_partitions: HashSet::new(),
            current_partition: None,
            stats: CheckpointStats::default(),
        }
    }

    /// Load checkpoint from file.
    ///
    /// Returns `Ok(None)` if file doesn't exist or is corrupted.
    /// Corrupted checkpoints are logged as warnings but don't cause errors.
    pub fn load(path: &Path) -> Result<Option<Self>, CheckpointError> {
        if !path.exists() {
            debug!("No checkpoint file at {}", path.display());
            return Ok(None);
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read checkpoint file: {}", e);
                return Ok(None);
            }
        };

        match serde_json::from_str::<Self>(&content) {
            Ok(checkpoint) => {
                info!(
                    "Loaded checkpoint: {}/{} partitions completed",
                    checkpoint.completed_partitions.len(),
                    checkpoint.stats.partitions_processed
                );
                Ok(Some(checkpoint))
            }
            Err(e) => {
                warn!("Corrupted checkpoint file, will start fresh: {}", e);
                Ok(None)
            }
        }
    }

    /// Save checkpoint to file atomically.
    ///
    /// Uses write-to-temp-then-rename pattern for crash safety.
    pub fn save(&self, path: &Path) -> Result<(), CheckpointError> {
        let content = serde_json::to_string_pretty(self)?;

        // Write to temp file first
        let temp_path = path.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&temp_path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }

        // Atomic rename
        fs::rename(&temp_path, path)?;

        debug!(
            "Saved checkpoint: {} partitions completed",
            self.completed_partitions.len()
        );
        Ok(())
    }

    /// Delete checkpoint file.
    ///
    /// Called on successful completion to clean up.
    pub fn delete(path: &Path) -> Result<(), CheckpointError> {
        if path.exists() {
            fs::remove_file(path)?;
            info!("Deleted checkpoint file after successful completion");
        }
        Ok(())
    }

    /// Check if checkpoint is valid for resuming.
    pub fn is_resumable(&self, manifest_hash: &str, qdrant_url: &str) -> ResumeValidation {
        // Check if already completed
        if self.state == IndexState::Completed {
            return ResumeValidation::AlreadyCompleted;
        }

        // Check if failed
        if let IndexState::Failed { error } = &self.state {
            return ResumeValidation::PreviousFailed {
                error: error.clone(),
            };
        }

        // Check manifest hash
        if self.manifest_hash != manifest_hash {
            return ResumeValidation::ManifestChanged {
                old_hash: self.manifest_hash.clone(),
                new_hash: manifest_hash.to_string(),
            };
        }

        // Check Qdrant URL
        if self.qdrant_url != qdrant_url {
            return ResumeValidation::QdrantUrlMismatch {
                old_url: self.qdrant_url.clone(),
                new_url: qdrant_url.to_string(),
            };
        }

        ResumeValidation::Valid
    }

    /// Mark a partition as starting.
    ///
    /// Called before processing a partition. If indexing is interrupted,
    /// this partition will be re-indexed on resume.
    pub fn start_partition(&mut self, partition_id: &str) {
        self.current_partition = Some(partition_id.to_string());
        self.last_updated = Utc::now();
    }

    /// Mark a partition as completed and accumulate stats.
    ///
    /// Called after successfully indexing a partition.
    pub fn complete_partition(&mut self, partition_id: &str, stats: &crate::IndexStats) {
        self.completed_partitions.insert(partition_id.to_string());
        self.current_partition = None;
        self.stats.accumulate(stats);
        self.last_updated = Utc::now();
    }

    /// Mark indexing as completed.
    pub fn mark_completed(&mut self) {
        self.state = IndexState::Completed;
        self.current_partition = None;
        self.last_updated = Utc::now();
    }

    /// Mark indexing as failed.
    pub fn mark_failed(&mut self, error: String) {
        self.state = IndexState::Failed { error };
        self.last_updated = Utc::now();
    }

    /// Get the number of completed partitions.
    pub fn completed_count(&self) -> usize {
        self.completed_partitions.len()
    }

    /// Check if a partition has been completed.
    pub fn is_partition_completed(&self, partition_id: &str) -> bool {
        self.completed_partitions.contains(partition_id)
    }
}

/// Compute SHA-256 hash of a file's content.
///
/// Used to detect if the manifest has changed since the checkpoint was created.
pub fn compute_manifest_hash(path: &Path) -> Result<String, CheckpointError> {
    let content = fs::read(path)?;
    let hash = Sha256::digest(&content);
    Ok(format!("{:x}", hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_checkpoint_creation() {
        let checkpoint = IndexCheckpoint::new(
            "test-repo".to_string(),
            "abc123".to_string(),
            "http://localhost:6334".to_string(),
        );

        assert_eq!(checkpoint.version, CHECKPOINT_VERSION);
        assert_eq!(checkpoint.repo_id, "test-repo");
        assert_eq!(checkpoint.state, IndexState::InProgress);
        assert!(checkpoint.completed_partitions.is_empty());
        assert!(checkpoint.current_partition.is_none());
    }

    #[test]
    fn test_checkpoint_save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");

        let mut checkpoint = IndexCheckpoint::new(
            "test-repo".to_string(),
            "abc123".to_string(),
            "http://localhost:6334".to_string(),
        );

        checkpoint.completed_partitions.insert("part1".to_string());
        checkpoint.completed_partitions.insert("part2".to_string());
        checkpoint.current_partition = Some("part3".to_string());

        checkpoint.save(&path).unwrap();

        let loaded = IndexCheckpoint::load(&path).unwrap().unwrap();

        assert_eq!(loaded.repo_id, "test-repo");
        assert_eq!(loaded.manifest_hash, "abc123");
        assert_eq!(loaded.completed_partitions.len(), 2);
        assert!(loaded.completed_partitions.contains("part1"));
        assert!(loaded.completed_partitions.contains("part2"));
        assert_eq!(loaded.current_partition, Some("part3".to_string()));
    }

    #[test]
    fn test_manifest_hash_validation() {
        let checkpoint = IndexCheckpoint::new(
            "test-repo".to_string(),
            "abc123".to_string(),
            "http://localhost:6334".to_string(),
        );

        // Same hash should be valid
        assert!(matches!(
            checkpoint.is_resumable("abc123", "http://localhost:6334"),
            ResumeValidation::Valid
        ));

        // Different hash should fail
        assert!(matches!(
            checkpoint.is_resumable("xyz789", "http://localhost:6334"),
            ResumeValidation::ManifestChanged { .. }
        ));

        // Different URL should fail
        assert!(matches!(
            checkpoint.is_resumable("abc123", "http://other:6334"),
            ResumeValidation::QdrantUrlMismatch { .. }
        ));
    }

    #[test]
    fn test_partition_completion_tracking() {
        let mut checkpoint = IndexCheckpoint::new(
            "test-repo".to_string(),
            "abc123".to_string(),
            "http://localhost:6334".to_string(),
        );

        assert!(!checkpoint.is_partition_completed("part1"));

        checkpoint.start_partition("part1");
        assert_eq!(checkpoint.current_partition, Some("part1".to_string()));
        assert!(!checkpoint.is_partition_completed("part1"));

        let stats = crate::IndexStats {
            total_processed: 100,
            total_indexed: 90,
            total_skipped: 5,
            total_failed: 5,
            semantic_indexed: 90,
            code_indexed: 90,
            partitions_processed: 1,
            nodes_per_partition_avg: 100.0,
            estimated_peak_memory_bytes: 1024,
        };

        checkpoint.complete_partition("part1", &stats);
        assert!(checkpoint.is_partition_completed("part1"));
        assert!(checkpoint.current_partition.is_none());
        assert_eq!(checkpoint.stats.total_indexed, 90);
    }

    #[test]
    fn test_corrupted_checkpoint_handling() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");

        // Write invalid JSON
        fs::write(&path, "{ invalid json }").unwrap();

        // Should return None, not error
        let result = IndexCheckpoint::load(&path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_missing_checkpoint() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let result = IndexCheckpoint::load(&path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_completed_state_not_resumable() {
        let mut checkpoint = IndexCheckpoint::new(
            "test-repo".to_string(),
            "abc123".to_string(),
            "http://localhost:6334".to_string(),
        );

        checkpoint.mark_completed();

        assert!(matches!(
            checkpoint.is_resumable("abc123", "http://localhost:6334"),
            ResumeValidation::AlreadyCompleted
        ));
    }

    #[test]
    fn test_failed_state_resumable() {
        let mut checkpoint = IndexCheckpoint::new(
            "test-repo".to_string(),
            "abc123".to_string(),
            "http://localhost:6334".to_string(),
        );

        checkpoint.mark_failed("Connection lost".to_string());

        match checkpoint.is_resumable("abc123", "http://localhost:6334") {
            ResumeValidation::PreviousFailed { error } => {
                assert_eq!(error, "Connection lost");
            }
            _ => panic!("Expected PreviousFailed"),
        }
    }
}
