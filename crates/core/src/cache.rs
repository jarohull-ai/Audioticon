use anyhow::{Context, Result};
use crate::audio::build_ffmpeg_filter_chain;
use crate::types::TextSegment;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const CACHE_VERSION: u32 = 4;

/// Default maximum cache size: 2 GB
pub const DEFAULT_MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Computes a unique SHA-256 fingerprint for a text segment.
/// Incorporates expressive actor tags, acoustic filter chain, and CACHE_VERSION so
/// changing DSP presets or actor delivery tags automatically invalidates outdated cache.
pub fn compute_segment_hash(segment: &TextSegment, voice: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CACHE_VERSION.to_le_bytes());
    hasher.update(segment.text.trim().as_bytes());
    hasher.update(segment.rate_mod.to_le_bytes());
    hasher.update(segment.pitch_mod.to_le_bytes());
    hasher.update(segment.volume_mod.to_le_bytes());

    // Hash sorted actor tags
    let mut tags = segment.actor_tags.clone();
    tags.sort();
    for tag in tags {
        hasher.update(format!("{:?}", tag).as_bytes());
    }

    // Hash the exact FFmpeg filter chain rather than just the preset name
    let filter_chain = build_ffmpeg_filter_chain(segment.filter.as_deref());
    hasher.update(filter_chain.as_bytes());

    let voice_to_use = segment.voice.as_deref().unwrap_or(voice);
    hasher.update(voice_to_use.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheStats {
    pub total_files: usize,
    pub total_bytes: u64,
    pub oldest_file_time: Option<SystemTime>,
    pub newest_file_time: Option<SystemTime>,
}

impl CacheStats {
    pub fn size_mb(&self) -> f64 {
        self.total_bytes as f64 / (1024.0 * 1024.0)
    }

    pub fn size_gb(&self) -> f64 {
        self.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }
}

/// Computes disk statistics for the cache directory
pub fn get_cache_stats(cache_dir: &Path) -> Result<CacheStats> {
    if !cache_dir.exists() {
        return Ok(CacheStats {
            total_files: 0,
            total_bytes: 0,
            oldest_file_time: None,
            newest_file_time: None,
        });
    }

    let mut total_files = 0;
    let mut total_bytes = 0u64;
    let mut oldest: Option<SystemTime> = None;
    let mut newest: Option<SystemTime> = None;

    for entry in fs::read_dir(cache_dir).with_context(|| format!("Błąd odczytu {:?}", cache_dir))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Ok(meta) = entry.metadata() {
                total_files += 1;
                total_bytes += meta.len();
                let mtime = meta.modified().unwrap_or_else(|_| SystemTime::now());
                oldest = Some(oldest.map_or(mtime, |cur| cur.min(mtime)));
                newest = Some(newest.map_or(mtime, |cur| cur.max(mtime)));
            }
        }
    }

    Ok(CacheStats {
        total_files,
        total_bytes,
        oldest_file_time: oldest,
        newest_file_time: newest,
    })
}

/// Parses human readable size strings like "500MB", "2GB", "1.5G", "800M" into bytes.
pub fn parse_size_string(s: &str) -> Result<u64> {
    let clean = s.trim().to_uppercase();
    if clean.ends_with("GB") || clean.ends_with('G') {
        let num_str = clean.trim_end_matches("GB").trim_end_matches('G').trim();
        let val: f64 = num_str
            .parse()
            .with_context(|| format!("Niepoprawna wartość rozmiaru GB: {}", s))?;
        Ok((val * 1024.0 * 1024.0 * 1024.0) as u64)
    } else if clean.ends_with("MB") || clean.ends_with('M') {
        let num_str = clean.trim_end_matches("MB").trim_end_matches('M').trim();
        let val: f64 = num_str
            .parse()
            .with_context(|| format!("Niepoprawna wartość rozmiaru MB: {}", s))?;
        Ok((val * 1024.0 * 1024.0) as u64)
    } else if clean.ends_with("KB") || clean.ends_with('K') {
        let num_str = clean.trim_end_matches("KB").trim_end_matches('K').trim();
        let val: f64 = num_str
            .parse()
            .with_context(|| format!("Niepoprawna wartość rozmiaru KB: {}", s))?;
        Ok((val * 1024.0) as u64)
    } else if clean.ends_with('B') {
        let num_str = clean.trim_end_matches('B').trim();
        let val: u64 = num_str
            .parse()
            .with_context(|| format!("Niepoprawna wartość rozmiaru bajtów: {}", s))?;
        Ok(val)
    } else {
        // Default to megabytes if only a number is provided
        let val: f64 = clean
            .parse()
            .with_context(|| format!("Niepoprawny format rozmiaru: {}", s))?;
        Ok((val * 1024.0 * 1024.0) as u64)
    }
}

/// Prunes cache directory using Least Recently Used (LRU) policy when total size exceeds max_bytes.
/// Protects authentic user punch-in recordings (files starting with 'recorded_') from being deleted!
/// Deletes oldest TTS cache chunks until total cache size is reduced to 80% of max_bytes (hysteresis).
/// Returns (pruned_files_count, bytes_freed).
pub fn prune_cache_lru(cache_dir: &Path, max_bytes: u64) -> Result<(usize, u64)> {
    if !cache_dir.exists() || max_bytes == 0 {
        return Ok((0, 0));
    }

    struct CacheItem {
        path: PathBuf,
        size: u64,
        modified: SystemTime,
    }

    let mut items: Vec<CacheItem> = Vec::new();
    let mut current_total_bytes = 0u64;

    for entry in fs::read_dir(cache_dir).with_context(|| format!("Błąd odczytu {:?}", cache_dir))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            // CRITICAL: Never delete user punch-in voice recordings!
            if file_name.starts_with("recorded_") {
                continue;
            }

            if let Ok(meta) = entry.metadata() {
                let size = meta.len();
                current_total_bytes += size;
                let modified = meta.modified().unwrap_or_else(|_| SystemTime::now());
                items.push(CacheItem {
                    path,
                    size,
                    modified,
                });
            }
        }
    }

    if current_total_bytes <= max_bytes {
        return Ok((0, 0));
    }

    // Sort oldest modified first
    items.sort_by_key(|item| item.modified);

    // Hysteresis: prune until 80% of limit
    let target_bytes = (max_bytes as f64 * 0.80) as u64;
    let mut pruned_count = 0;
    let mut freed_bytes = 0u64;

    for item in items {
        if current_total_bytes <= target_bytes {
            break;
        }
        if fs::remove_file(&item.path).is_ok() {
            current_total_bytes = current_total_bytes.saturating_sub(item.size);
            freed_bytes += item.size;
            pruned_count += 1;
        }
    }

    Ok((pruned_count, freed_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ActorTag;

    #[test]
    fn test_hash_consistency() {
        let seg1 = TextSegment {
            text: "Na biurku leżał notes.".to_string(),
            rate_mod: -6,
            pitch_mod: -4,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![ActorTag::Whisper],
            voice: None,
        };
        let seg2 = seg1.clone();
        assert_eq!(
            compute_segment_hash(&seg1, "pl-PL-MarekNeural"),
            compute_segment_hash(&seg2, "pl-PL-MarekNeural")
        );
    }

    #[test]
    fn test_hash_sensitivity() {
        let seg1 = TextSegment {
            text: "Na biurku leżał notes.".to_string(),
            rate_mod: -6,
            pitch_mod: -4,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![],
            voice: None,
        };
        let mut seg2 = seg1.clone();
        seg2.actor_tags.push(ActorTag::Whisper);

        assert_ne!(
            compute_segment_hash(&seg1, "pl-PL-MarekNeural"),
            compute_segment_hash(&seg2, "pl-PL-MarekNeural")
        );

        let mut seg3 = seg1.clone();
        seg3.rate_mod = -14;
        assert_ne!(
            compute_segment_hash(&seg1, "pl-PL-MarekNeural"),
            compute_segment_hash(&seg3, "pl-PL-MarekNeural")
        );

        let mut seg4 = seg1.clone();
        seg4.filter = Some("cathedral".to_string());
        assert_ne!(
            compute_segment_hash(&seg1, "pl-PL-MarekNeural"),
            compute_segment_hash(&seg4, "pl-PL-MarekNeural")
        );

        let mut seg5 = seg1.clone();
        seg5.voice = Some("pl-PL-ZofiaNeural".to_string());
        assert_ne!(
            compute_segment_hash(&seg1, "pl-PL-MarekNeural"),
            compute_segment_hash(&seg5, "pl-PL-MarekNeural")
        );
    }

    #[test]
    fn test_parse_size_string() {
        assert_eq!(parse_size_string("500MB").unwrap(), 500 * 1024 * 1024);
        assert_eq!(parse_size_string("1GB").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size_string("2G").unwrap(), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size_string("1024KB").unwrap(), 1024 * 1024);
        assert_eq!(parse_size_string("2048B").unwrap(), 2048);
        assert_eq!(parse_size_string("250").unwrap(), 250 * 1024 * 1024); // default MB
        assert!(parse_size_string("invalid_xyz").is_err());
    }

    #[test]
    fn test_cache_stats_and_lru_pruning() {
        let tmp = std::env::temp_dir().join(format!("test_cache_lru_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);

        // Create 4 simulated TTS chunk files (each 1000 bytes)
        let f1 = tmp.join("chunk_1.wav");
        let f2 = tmp.join("chunk_2.wav");
        let f3 = tmp.join("chunk_3.wav");
        let f_rec = tmp.join("recorded_punchin_user.wav");

        fs::write(&f1, vec![0u8; 1000]).unwrap();
        fs::write(&f2, vec![0u8; 1000]).unwrap();
        fs::write(&f3, vec![0u8; 1000]).unwrap();
        // User punch-in recording must NEVER be deleted by LRU
        fs::write(&f_rec, vec![0u8; 1000]).unwrap();

        let stats = get_cache_stats(&tmp).unwrap();
        assert_eq!(stats.total_files, 4);
        assert_eq!(stats.total_bytes, 4000);

        // Prune with limit = 2500 bytes.
        // Chunks are 3000 bytes + 1000 bytes recorded.
        // It should prune oldest chunks, reducing below 2500 * 0.8 = 2000 bytes.
        // Crucially, 'recorded_punchin_user.wav' must remain intact.
        let (pruned_count, bytes_freed) = prune_cache_lru(&tmp, 2500).unwrap();
        assert!(pruned_count >= 1, "Should prune at least 1 file");
        assert!(bytes_freed >= 1000, "Should free at least 1000 bytes");
        assert!(f_rec.exists(), "Punch-in file must NEVER be deleted by LRU!");

        let _ = fs::remove_dir_all(&tmp);
    }
}
