//! Progress feedback utilities for CLI commands
//!
//! Provides spinners and progress bars for long-running operations.
//! All progress output is suppressed when --quiet flag is set.

use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// Create a spinner with a message
pub fn spinner(message: &str, quiet: bool) -> Option<ProgressBar> {
    if quiet {
        return None;
    }

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg}")
            .expect("Invalid spinner template"),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    Some(pb)
}

/// Create a progress bar with a known total
#[allow(dead_code)]
pub fn progress_bar(total: u64, message: &str, quiet: bool) -> Option<ProgressBar> {
    if quiet {
        return None;
    }

    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%)")
            .expect("Invalid progress bar template")
            .progress_chars("█▓░"),
    );
    pb.set_message(message.to_string());
    Some(pb)
}

/// Create a progress bar for download/processing with bytes
#[allow(dead_code)]
pub fn bytes_bar(total: u64, message: &str, quiet: bool) -> Option<ProgressBar> {
    if quiet {
        return None;
    }

    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})")
            .expect("Invalid bytes bar template")
            .progress_chars("█▓░"),
    );
    pb.set_message(message.to_string());
    Some(pb)
}

/// Finish a spinner with a success message
pub fn finish_spinner(pb: Option<ProgressBar>, message: &str) {
    if let Some(pb) = pb {
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{prefix:.green} {msg}")
                .expect("Invalid finish template"),
        );
        pb.set_prefix("✓");
        pb.finish_with_message(message.to_string());
    }
}

/// Finish a spinner with a warning message
pub fn finish_spinner_warn(pb: Option<ProgressBar>, message: &str) {
    if let Some(pb) = pb {
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{prefix:.yellow} {msg}")
                .expect("Invalid warn template"),
        );
        pb.set_prefix("!");
        pb.finish_with_message(message.to_string());
    }
}

/// Finish a spinner with an error message
#[allow(dead_code)]
pub fn finish_spinner_error(pb: Option<ProgressBar>, message: &str) {
    if let Some(pb) = pb {
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{prefix:.red} {msg}")
                .expect("Invalid error template"),
        );
        pb.set_prefix("✗");
        pb.finish_with_message(message.to_string());
    }
}

/// Finish a progress bar
#[allow(dead_code)]
pub fn finish_progress(pb: Option<ProgressBar>) {
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_quiet_returns_none() {
        let pb = spinner("test", true);
        assert!(pb.is_none());
    }

    #[test]
    fn test_spinner_not_quiet_returns_some() {
        let pb = spinner("test", false);
        assert!(pb.is_some());
        if let Some(pb) = pb {
            pb.finish();
        }
    }

    #[test]
    fn test_progress_bar_quiet_returns_none() {
        let pb = progress_bar(100, "test", true);
        assert!(pb.is_none());
    }

    #[test]
    fn test_finish_spinner_handles_none() {
        // Should not panic
        finish_spinner(None, "done");
        finish_spinner_warn(None, "warning");
        finish_spinner_error(None, "error");
    }

    #[test]
    fn test_finish_progress_handles_none() {
        // Should not panic
        finish_progress(None);
    }
}
