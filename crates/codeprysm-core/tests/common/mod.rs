//! Common test utilities for integration tests.
//!
//! This module provides graph validation utilities shared across
//! integration test files.

#![allow(dead_code)]
#![allow(unused_imports)]

pub mod graph_validator;
pub mod test_repos;

// Re-export commonly used items
pub use graph_validator::{
    assert_contains_edge, assert_entity_exists, check_min_counts, compute_stats, validate_all,
    validate_relational, validate_semantic, validate_structural, GraphStats, GraphValidationResult,
};
pub use test_repos::{RepoCache, RepoError, TestRepo};
