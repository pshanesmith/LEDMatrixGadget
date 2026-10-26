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
pub fn write_atomic(path: &std::path::Path, text: &str, tmp_suffix: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(tmp_suffix);
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}
