//! Set one value in a TOML config file safely: idempotent, repairs duplicate sections.
//!
//! - Creates the `[section]` if missing, otherwise changes (or adds) just that key.
//! - Merges accidentally duplicated `[sections]` (later values win), keeping other lines and comments.
//! - Validates the result with a TOML parser before touching the file; keeps a `.bak` copy.

use std::path::Path;

use anyhow::{Context, Result};

fn header_name(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('[')?;
    let close = rest.find(']')?;
    let name = &rest[..close];
    let after = rest[close + 1..].trim_start();
    (!name.is_empty() && !name.contains('[') && (after.is_empty() || after.starts_with('#'))).then_some(name)
}

fn key_name(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let n = t.bytes().take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')).count();
    (n > 0 && t[n..].trim_start().starts_with('=')).then(|| &t[..n])
}

/// Numbers and booleans stay bare; anything else is quoted.
pub fn format_value(raw: &str) -> String {
    let is_number = {
        let s = raw.strip_prefix('-').unwrap_or(raw);
        let (int, frac) = match s.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (s, None),
        };
        let digits = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit());
        digits(int) && frac.is_none_or(digits)
    };
    if is_number || raw == "true" || raw == "false" {
        return raw.to_string();
    }
    if raw.starts_with(['"', '\'', '[']) {
        return raw.to_string();
    }
    format!("\"{}\"", raw.replace('"', "\\\""))
}

pub fn update(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut preamble: Vec<String> = Vec::new();
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<usize> = None;
    for line in text.lines() {
        if let Some(name) = header_name(line) {
            let name = name.trim();
            current = Some(match sections.iter().position(|(n, _)| n == name) {
                Some(i) => i,
                None => {
                    sections.push((name.to_string(), Vec::new()));
                    sections.len() - 1
                }
            });
            continue;
        }
        match current {
            None => preamble.push(line.to_string()),
            Some(i) => sections[i].1.push(line.to_string()),
        }
    }
    // merge: within a section a later "key = ..." replaces an earlier one (keep the first position)
    for (_, lines) in sections.iter_mut() {
        let mut seen: Vec<(String, usize)> = Vec::new();
        let mut out: Vec<String> = Vec::new();
        for line in lines.drain(..) {
            match key_name(&line).map(String::from) {
                Some(k) if seen.iter().any(|(s, _)| *s == k) => {
                    let idx = seen.iter().find(|(s, _)| *s == k).unwrap().1;
                    out[idx] = line;
                }
                Some(k) => {
                    seen.push((k, out.len()));
                    out.push(line);
                }
                None => out.push(line),
            }
        }
        *lines = out;
    }
    let idx = match sections.iter().position(|(n, _)| n == section) {
        Some(i) => i,
        None => {
            sections.push((section.to_string(), Vec::new()));
            sections.len() - 1
        }
    };
    let lines = &mut sections[idx].1;
    let new_line = format!("{key} = {}", format_value(value));
    match lines.iter().position(|l| key_name(l) == Some(key)) {
        Some(i) => lines[i] = new_line,
        None => {
            // insert after the last non-blank line of the section
            let mut pos = lines.len();
            while pos > 0 && lines[pos - 1].trim().is_empty() {
                pos -= 1;
            }
            lines.insert(pos, new_line);
        }
    }
    let mut out = preamble;
    for (name, body) in sections {
        while out.last().is_some_and(|l| l.trim().is_empty()) {
            out.pop();
        }
        out.push(String::new());
        out.push(format!("[{name}]"));
        out.extend(body);
    }
    format!("{}\n", out.join("\n").trim_matches('\n'))
}

/// Edit the file in place; returns the message to print. Nothing is touched if the result is invalid.
pub fn run(path: &Path, section: &str, key: &str, value: &str) -> Result<String> {
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let new_text = update(&text, section, key, value);
    new_text.parse::<toml::Table>().context("the result would not be valid TOML; nothing changed")?;
    let with_suffix = |s: &str| {
        let mut p = path.as_os_str().to_owned();
        p.push(s);
        std::path::PathBuf::from(p)
    };
    std::fs::copy(path, with_suffix(".bak"))?;
    let tmp = with_suffix(".tmp");
    std::fs::write(&tmp, &new_text)?;
    std::fs::set_permissions(&tmp, std::fs::metadata(path)?.permissions())?;
    std::fs::rename(&tmp, path)?;
    Ok(format!("set [{section}] {key} = {} in {}", format_value(value), path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROKEN: &str = "# my comment
[location]
latitude = 40.748
longitude = -73.994

[homeassistant]
url = \"http://homeassistant:8123\"
gates = [\"a\", \"b\"]

[display]
pwm_bits = 10

[display]
pwm_bits = 9

[display]
pwm_bits = 10
";

    fn parsed(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    #[test]
    fn repairs_duplicates_and_sets_value() {
        let out = update(BROKEN, "display", "pwm_bits", "9");
        let data = parsed(&out);
        assert_eq!(data["display"].as_table().unwrap().len(), 1);
        assert_eq!(data["display"]["pwm_bits"].as_integer(), Some(9));
        assert_eq!(data["location"]["latitude"].as_float(), Some(40.748));
        assert_eq!(data["homeassistant"]["gates"].as_array().unwrap().len(), 2);
        assert_eq!(out.matches("[display]").count(), 1);
        assert!(out.starts_with("# my comment"));
    }

    #[test]
    fn idempotent_and_adds_new_keys_and_sections() {
        let once = update(BROKEN, "display", "pwm_bits", "10");
        assert_eq!(update(&once, "display", "pwm_bits", "10"), once);
        let more = update(&once, "display", "limit_refresh_hz", "150");
        let display = &parsed(&more)["display"];
        assert_eq!(display["pwm_bits"].as_integer(), Some(10));
        assert_eq!(display["limit_refresh_hz"].as_integer(), Some(150));
        let fresh = update("[location]\nlatitude = 1.0\nlongitude = 2.0\n", "display", "pwm_bits", "9");
        assert_eq!(parsed(&fresh)["display"]["pwm_bits"].as_integer(), Some(9));
    }

    #[test]
    fn strings_and_booleans_formatting() {
        let out = update("", "display", "hardware_mapping", "adafruit-hat");
        assert_eq!(parsed(&out)["display"]["hardware_mapping"].as_str(), Some("adafruit-hat"));
        let out = update("", "display", "drop_privileges", "false");
        assert_eq!(parsed(&out)["display"]["drop_privileges"].as_bool(), Some(false));
        assert_eq!(format_value("-1.5"), "-1.5");
        assert_eq!(format_value("[1, 2]"), "[1, 2]");
        assert_eq!(format_value("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn writes_backup_and_refuses_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("c.toml");
        std::fs::write(&f, BROKEN).unwrap();
        run(&f, "display", "pwm_bits", "10").unwrap();
        assert_eq!(parsed(&std::fs::read_to_string(&f).unwrap())["display"]["pwm_bits"].as_integer(), Some(10));
        assert_eq!(std::fs::read_to_string(dir.path().join("c.toml.bak")).unwrap(), BROKEN);

        // a value that cannot be TOML leaves the file alone
        let before = std::fs::read_to_string(&f).unwrap();
        assert!(run(&f, "display", "pwm_bits", "[unclosed").is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), before);
    }
}
