//! GitHub repository management for real-world integration tests.
//!
//! This module handles cloning, caching, and version pinning of GitHub
//! repositories used for Tier 2 integration testing.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Configuration for a test repository
#[derive(Debug, Clone)]
pub struct TestRepo {
    /// Repository owner/name (e.g., "pallets/flask")
    pub repo: &'static str,
    /// Git commit SHA to pin to for reproducibility
    pub commit: &'static str,
    /// Subdirectory to analyze (None = entire repo)
    pub subdir: Option<&'static str>,
    /// Language for this repository
    pub language: &'static str,
    /// Human-readable description
    pub description: &'static str,
}

impl TestRepo {
    /// Get the GitHub clone URL
    pub fn clone_url(&self) -> String {
        format!("https://github.com/{}.git", self.repo)
    }

    /// Get a unique cache directory name
    pub fn cache_name(&self) -> String {
        let safe_name = self.repo.replace('/', "_");
        format!("{}_{}", safe_name, &self.commit[..8])
    }

    /// Get the path to analyze (repo root or subdirectory)
    pub fn analysis_path(&self, repo_path: &Path) -> PathBuf {
        match self.subdir {
            Some(subdir) => repo_path.join(subdir),
            None => repo_path.to_path_buf(),
        }
    }
}

/// Test repositories for each supported language.
///
/// Selection criteria:
/// - Well-known, popular projects
/// - Reasonable size (<50MB source)
/// - Representative code patterns
/// - Stable API/structure at pinned commit
pub mod repos {
    use super::TestRepo;

    /// Python: Flask web framework (subset)
    pub const PYTHON: TestRepo = TestRepo {
        repo: "pallets/flask",
        commit: "735a4701d6d5e848241e7d7535db898efb62d400", // v3.0.0
        subdir: Some("src/flask"),
        language: "python",
        description: "Flask web framework core",
    };

    /// JavaScript: Express.js web framework
    pub const JAVASCRIPT: TestRepo = TestRepo {
        repo: "expressjs/express",
        commit: "8368dc178af16b91b576c4c1d135f701a0007e5d", // v4.18.2
        subdir: Some("lib"),
        language: "javascript",
        description: "Express.js core library",
    };

    /// TypeScript: TypeORM (subset)
    pub const TYPESCRIPT: TestRepo = TestRepo {
        repo: "typeorm/typeorm",
        commit: "73fda419e4647c10377b28bd975171156c285693", // v0.3.28
        subdir: Some("src/entity-manager"),
        language: "typescript",
        description: "TypeORM entity manager",
    };

    /// C: hiredis Redis client library
    pub const C: TestRepo = TestRepo {
        repo: "redis/hiredis",
        commit: "ccad7ebaf99310957004661d1c5f82d2a33ebd10", // v1.3.0
        subdir: None,
        language: "c",
        description: "hiredis Redis client library",
    };

    /// C++: JSON for Modern C++
    pub const CPP: TestRepo = TestRepo {
        repo: "nlohmann/json",
        commit: "9cca280a4d0ccf0c08f47a99aa71d1b0e52f8d03", // v3.11.3
        subdir: Some("include/nlohmann"),
        language: "cpp",
        description: "nlohmann JSON library",
    };

    /// C#: Newtonsoft.Json (subset)
    pub const CSHARP: TestRepo = TestRepo {
        repo: "JamesNK/Newtonsoft.Json",
        commit: "4e13299d4b0ec96bd4df9954ef646bd2d1b5bf2a", // v13.0.4
        subdir: Some("Src/Newtonsoft.Json"),
        language: "csharp",
        description: "Newtonsoft.Json library",
    };

    /// Go: Echo web framework
    pub const GO: TestRepo = TestRepo {
        repo: "labstack/echo",
        commit: "6392cb459842d2c1747902ec2a1809c1387df5d8", // v4.14.0
        subdir: None,
        language: "go",
        description: "Echo web framework",
    };

    /// Rust: Serde (subset)
    pub const RUST: TestRepo = TestRepo {
        repo: "serde-rs/serde",
        commit: "a866b336f14aa57a07f0d0be9f8762746e64ecb4", // v1.0.228
        subdir: Some("serde/src"),
        language: "rust",
        description: "Serde serialization core",
    };

    /// Get all test repositories
    pub fn all() -> Vec<&'static TestRepo> {
        vec![
            &PYTHON,
            &JAVASCRIPT,
            &TYPESCRIPT,
            &C,
            &CPP,
            &CSHARP,
            &GO,
            &RUST,
        ]
    }

    /// Get test repository by language
    pub fn by_language(lang: &str) -> Option<&'static TestRepo> {
        all().into_iter().find(|r| r.language == lang)
    }
}

/// Repository cache manager
pub struct RepoCache {
    /// Base directory for cached repositories
    cache_dir: PathBuf,
}

impl RepoCache {
    /// Create a new cache manager
    pub fn new() -> std::io::Result<Self> {
        // Use target/test-repos as cache directory
        let cache_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-repos");

        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self { cache_dir })
    }

    /// Get or clone a repository
    pub fn get_repo(&self, repo: &TestRepo) -> Result<PathBuf, RepoError> {
        let repo_path = self.cache_dir.join(repo.cache_name());

        if repo_path.exists() {
            // Verify the commit is correct
            if self.verify_commit(&repo_path, repo.commit)? {
                return Ok(repo.analysis_path(&repo_path));
            }
            // Wrong commit, remove and re-clone
            std::fs::remove_dir_all(&repo_path).map_err(|e| RepoError::Io(e.to_string()))?;
        }

        // Clone the repository
        self.clone_repo(repo, &repo_path)?;

        // Checkout specific commit
        self.checkout_commit(&repo_path, repo.commit)?;

        Ok(repo.analysis_path(&repo_path))
    }

    /// Clone repository to target path
    fn clone_repo(&self, repo: &TestRepo, target: &Path) -> Result<(), RepoError> {
        let output = Command::new("git")
            .args(["clone", "--depth", "1", &repo.clone_url()])
            .arg(target)
            .output()
            .map_err(|e| RepoError::Git(format!("Failed to run git: {}", e)))?;

        if !output.status.success() {
            // Try full clone for specific commit checkout
            let output = Command::new("git")
                .args(["clone", &repo.clone_url()])
                .arg(target)
                .output()
                .map_err(|e| RepoError::Git(format!("Failed to run git: {}", e)))?;

            if !output.status.success() {
                return Err(RepoError::Git(format!(
                    "Clone failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
        }

        Ok(())
    }

    /// Checkout specific commit
    fn checkout_commit(&self, repo_path: &Path, commit: &str) -> Result<(), RepoError> {
        // First fetch the commit if needed
        let fetch_output = Command::new("git")
            .current_dir(repo_path)
            .args(["fetch", "origin", commit])
            .output()
            .map_err(|e| RepoError::Git(format!("Failed to run git fetch: {}", e)))?;

        // Ignore fetch errors (might not be needed for shallow clone)
        let _ = fetch_output;

        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["checkout", commit])
            .output()
            .map_err(|e| RepoError::Git(format!("Failed to run git checkout: {}", e)))?;

        if !output.status.success() {
            return Err(RepoError::Git(format!(
                "Checkout failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Verify repository is at expected commit
    fn verify_commit(&self, repo_path: &Path, expected: &str) -> Result<bool, RepoError> {
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["rev-parse", "HEAD"])
            .output()
            .map_err(|e| RepoError::Git(format!("Failed to get commit: {}", e)))?;

        if !output.status.success() {
            return Ok(false);
        }

        let current = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(current.starts_with(expected) || expected.starts_with(&current[..8.min(current.len())]))
    }

    /// Clean all cached repositories
    pub fn clean(&self) -> std::io::Result<()> {
        if self.cache_dir.exists() {
            std::fs::remove_dir_all(&self.cache_dir)?;
        }
        Ok(())
    }
}

impl Default for RepoCache {
    fn default() -> Self {
        Self::new().expect("Failed to create repo cache")
    }
}

/// Errors that can occur during repository operations
#[derive(Debug)]
pub enum RepoError {
    /// Git command failed
    Git(String),
    /// IO error
    Io(String),
}

impl std::fmt::Display for RepoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepoError::Git(msg) => write!(f, "Git error: {}", msg),
            RepoError::Io(msg) => write!(f, "IO error: {}", msg),
        }
    }
}

impl std::error::Error for RepoError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_clone_url() {
        let repo = &repos::PYTHON;
        assert_eq!(repo.clone_url(), "https://github.com/pallets/flask.git");
    }

    #[test]
    fn test_repo_cache_name() {
        let repo = &repos::PYTHON;
        let name = repo.cache_name();
        assert!(name.starts_with("pallets_flask_"));
        assert!(name.len() > 15); // owner_name + 8 chars of SHA
    }

    #[test]
    fn test_all_repos_have_language() {
        for repo in repos::all() {
            assert!(!repo.language.is_empty());
            assert!(!repo.repo.is_empty());
            assert!(repo.commit.len() >= 8);
        }
    }
}
