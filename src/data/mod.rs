//! Background data feeds: weather, Home Assistant, and the one-time ZIP geocoder.

pub mod geocode;
pub mod homeassistant;
pub mod weather;

use chrono::{DateTime, FixedOffset};

/// `datetime.isoformat()`-style text: what the caches on disk hold.
pub fn iso(dt: &DateTime<FixedOffset>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
}

pub fn parse_iso(text: &str) -> Result<DateTime<FixedOffset>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(text)
}

/// Write a file by way of a temp file so a crash never leaves half of it.
///
/// The temp name is unique per process and call (`<path>.<pid>.<n>.tmp`): after a reload the old
/// feed's thread may still be finishing a write to the same cache while the new one starts, and
/// two writers must never share a temp file.
pub fn write_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.{n}.tmp", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp);
    let written = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_atomic_writes_never_mix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        let texts: Vec<String> = (0..8).map(|i| format!("{{\"writer\": {i}, \"pad\": \"{}\"}}", "x".repeat(20_000))).collect();
        std::thread::scope(|s| {
            for text in &texts {
                let path = &path;
                s.spawn(move || {
                    for _ in 0..20 {
                        write_atomic(path, text).unwrap();
                    }
                });
            }
        });
        let result = std::fs::read_to_string(&path).unwrap();
        assert!(texts.contains(&result), "the cache holds one writer's whole text");
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["cache.json"], "no temp files are left behind");
    }
}
