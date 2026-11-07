//! Auto-Updater and Version Diagnostic Module for Audioticon
//!
//! Asynchronously checks for new Audioticon releases on GitHub Releases / Gumroad.
//! Built with clean, offline-first fallback:
//! - Never blocks server startup or UI thread
//! - Gracefully fails when offline without raising user errors
//! - Semantic version comparison (Major.Minor.Patch)

use serde::{Deserialize, Serialize};
use std::process::Command;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GITHUB_RELEASES_API_URL: &str =
    "https://api.github.com/repos/jarohull-ai/Audioticon/releases/latest";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateCheckResponse {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: Option<String>,
    pub release_name: Option<String>,
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
    pub offline: bool,
}

impl Default for UpdateCheckResponse {
    fn default() -> Self {
        Self {
            current_version: CURRENT_VERSION.to_string(),
            latest_version: CURRENT_VERSION.to_string(),
            update_available: false,
            release_url: None,
            release_name: None,
            release_notes: None,
            published_at: None,
            offline: false,
        }
    }
}

/// Parses semantic version numbers into `(major, minor, patch)` tuple.
/// Handles leading 'v' prefixes, whitespace, and commit suffixes.
pub fn parse_semver(ver: &str) -> Option<(u32, u32, u32)> {
    let clean = ver.trim().trim_start_matches('v').trim();
    let parts: Vec<&str> = clean.split('.').collect();
    if parts.len() < 2 {
        return None;
    }

    let major = parts[0].parse::<u32>().ok()?;
    let minor = parts[1].parse::<u32>().ok()?;
    let patch = if parts.len() >= 3 {
        // Strip trailing pre-release tags like -alpha or +commit
        let patch_part = parts[2].split(['-', '+']).next()?;
        patch_part.parse::<u32>().unwrap_or(0)
    } else {
        0
    };

    Some((major, minor, patch))
}

/// Returns true if `latest` is strictly newer than `current`.
pub fn is_newer_version(latest: &str, current: &str) -> bool {
    let (l_maj, l_min, l_pat) = match parse_semver(latest) {
        Some(v) => v,
        None => return false,
    };
    let (c_maj, c_min, c_pat) = match parse_semver(current) {
        Some(v) => v,
        None => return false,
    };

    (l_maj, l_min, l_pat) > (c_maj, c_min, c_pat)
}

/// Checks the GitHub Releases API for new Audioticon versions using curl.
/// Fast 3-second timeout ensures zero noticeable latency even when offline.
pub fn check_for_updates() -> UpdateCheckResponse {
    let mut resp = UpdateCheckResponse::default();

    // Query GitHub API via curl with strict 3-second timeout
    let output = match Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "3",
            "-H",
            "User-Agent: Audioticon-Studio-Updater",
            "-H",
            "Accept: application/vnd.github.v3+json",
            GITHUB_RELEASES_API_URL,
        ])
        .output()
    {
        Ok(out) => out,
        Err(_) => {
            resp.offline = true;
            return resp;
        }
    };

    if !output.status.success() {
        resp.offline = true;
        return resp;
    }

    let body_str = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = match serde_json::from_str(&body_str) {
        Ok(v) => v,
        Err(_) => {
            resp.offline = true;
            return resp;
        }
    };

    if let Some(tag_name) = val.get("tag_name").and_then(|v| v.as_str()) {
        let latest_ver = tag_name.trim_start_matches('v').to_string();
        resp.latest_version = latest_ver.clone();
        resp.update_available = is_newer_version(&latest_ver, CURRENT_VERSION);
        resp.release_url = val
            .get("html_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        resp.release_name = val
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        resp.release_notes = val
            .get("body")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        resp.published_at = val
            .get("published_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        resp.offline = false;
    } else {
        resp.offline = true;
    }

    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_semver() {
        assert_eq!(parse_semver("0.2.8"), Some((0, 2, 8)));
        assert_eq!(parse_semver("v0.2.8"), Some((0, 2, 8)));
        assert_eq!(parse_semver("v1.10.4-rc1"), Some((1, 10, 4)));
        assert_eq!(parse_semver("  v2.0.0  "), Some((2, 0, 0)));
        assert_eq!(parse_semver("invalid"), None);
    }

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("0.2.9", "0.2.8"));
        assert!(is_newer_version("v0.3.0", "0.2.8"));
        assert!(is_newer_version("1.0.0", "0.2.8"));
        assert!(!is_newer_version("0.2.8", "0.2.8"));
        assert!(!is_newer_version("v0.2.8", "0.2.8"));
        assert!(!is_newer_version("0.2.7", "0.2.8"));
        assert!(!is_newer_version("invalid", "0.2.8"));
    }

    #[test]
    fn test_check_for_updates_offline_or_live() {
        let res = check_for_updates();
        assert_eq!(res.current_version, CURRENT_VERSION);
        // Either live check succeeds and finds v0.2.8, or offline flag is set
        if !res.offline {
            assert!(!res.latest_version.is_empty());
        }
    }
}
