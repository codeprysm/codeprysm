//! Configuration error types.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during configuration loading.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// Failed to read configuration file
    #[error("failed to read config file '{path}': {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse TOML configuration
    #[error("failed to parse config file '{path}': {source}")]
    ParseToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    /// Failed to serialize configuration
    #[error("failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// Failed to write configuration file
    #[error("failed to write config file '{path}': {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to create configuration directory
    #[error("failed to create config directory '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Home directory not found
    #[error("could not determine home directory")]
    NoHomeDir,

    /// Invalid configuration value
    #[error("invalid configuration value for '{key}': {message}")]
    InvalidValue { key: String, message: String },

    /// Workspace not found
    #[error("workspace '{name}' not found in configuration")]
    WorkspaceNotFound { name: String },

    /// Configuration validation error
    #[error("configuration validation failed: {0}")]
    ValidationError(String),
}

impl ConfigError {
    /// Create a new ReadFile error.
    pub fn read_file(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::ReadFile {
            path: path.into(),
            source,
        }
    }

    /// Create a new ParseToml error.
    pub fn parse_toml(path: impl Into<PathBuf>, source: toml::de::Error) -> Self {
        Self::ParseToml {
            path: path.into(),
            source,
        }
    }

    /// Create a new WriteFile error.
    pub fn write_file(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::WriteFile {
            path: path.into(),
            source,
        }
    }

    /// Create a new CreateDir error.
    pub fn create_dir(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::CreateDir {
            path: path.into(),
            source,
        }
    }

    /// Create a new InvalidValue error.
    pub fn invalid_value(key: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidValue {
            key: key.into(),
            message: message.into(),
        }
    }

    /// Create a new WorkspaceNotFound error.
    pub fn workspace_not_found(name: impl Into<String>) -> Self {
        Self::WorkspaceNotFound { name: name.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ConfigError::NoHomeDir;
        assert_eq!(err.to_string(), "could not determine home directory");

        let err = ConfigError::invalid_value("backend.type", "unknown backend 'foo'");
        assert!(err.to_string().contains("backend.type"));
        assert!(err.to_string().contains("unknown backend"));
    }

    #[test]
    fn test_workspace_not_found() {
        let err = ConfigError::workspace_not_found("my-project");
        assert!(err.to_string().contains("my-project"));
        assert!(err.to_string().contains("not found"));
    }
}
