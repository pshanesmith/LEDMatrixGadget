//! List Home Assistant locks, lights and gate candidates so they can be chosen for config.toml.
//! Read-only.
//!
//! The token is never accepted on the command line (shell history) and never printed. It is
//! read from, in order:
//!   1. the MATRIX_HA_TOKEN environment variable
//!   2. an env file: --env-file, else ./secrets.env, else /etc/matrix/secrets.env
//!   3. an interactive hidden prompt (only when stdin is a terminal)
//!
//! The base URL comes from MATRIX_HA_URL, --url, or the default.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::config::{parse_env_file, DEFAULT_HA_URL};

pub const ENV_FILES: [&str; 2] = ["secrets.env", "/etc/matrix/secrets.env"];
const GATE_DEVICE_CLASSES: [&str; 4] = ["door", "garage_door", "gate", "opening"];
/// Lights that are not room lighting: switch status-LED bars (Inovelli "RGB Indicator"),
/// access-point / switch LEDs. Counting them would inflate "lights on".
const NOISE_SUFFIXES: [&str; 3] = ["rgb_indicator", "_ap_led", "_led"];
pub const NOISE_GLOBS: [&str; 3] = ["light.*_rgb_indicator", "light.*_ap_led", "light.*_led"];

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub entity_id: String,
    pub name: String,
    pub state: String,
    pub device_class: String,
    /// Light groups and similar aggregates list their members; counting both the group and the
    /// members would double count.
    pub is_group: bool,
    pub is_noise: bool,
}

pub fn normalize_url(url: &str) -> String {
    let mut url = url.trim().to_string();
    if !url.contains("://") {
        url = format!("http://{url}");
    }
    url.trim_end_matches('/').to_string()
}

fn entry(state: &Value) -> Entry {
    let attrs = state.get("attributes").cloned().unwrap_or(Value::Null);
    let text = |k: &str| attrs.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let entity_id = state.get("entity_id").and_then(Value::as_str).unwrap_or("").to_string();
    let domain = entity_id.split('.').next().unwrap_or("");
    Entry {
        name: text("friendly_name"),
        state: state.get("state").and_then(Value::as_str).unwrap_or("").to_string(),
        device_class: text("device_class"),
        is_group: attrs.get("entity_id").is_some_and(Value::is_array),
        is_noise: domain == "light" && NOISE_SUFFIXES.iter().any(|s| entity_id.ends_with(s)),
        entity_id,
    }
}

pub struct Classified {
    pub locks: Vec<Entry>,
    pub lights: Vec<Entry>,
    pub gates: Vec<Entry>,
    pub other: Vec<Entry>,
}

/// Split raw `/api/states` output into locks, lights, gates and other cover/sensors.
pub fn classify(states: &[Value]) -> Classified {
    let mut c = Classified { locks: vec![], lights: vec![], gates: vec![], other: vec![] };
    for state in states {
        let e = entry(state);
        let domain = e.entity_id.split('.').next().unwrap_or("").to_string();
        match domain.as_str() {
            "lock" => c.locks.push(e),
            "light" => c.lights.push(e),
            "cover" | "binary_sensor" => {
                let text = format!("{} {}", e.entity_id, e.name).to_lowercase();
                if text.contains("gate") {
                    c.gates.push(e);
                } else if GATE_DEVICE_CLASSES.contains(&e.device_class.as_str()) {
                    c.other.push(e);
                }
            }
            _ => {}
        }
    }
    for list in [&mut c.locks, &mut c.lights, &mut c.gates, &mut c.other] {
        list.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
    }
    c
}

/// `(url, token)` or the message to exit with.
pub fn load_settings(env_file: Option<&str>, environ: &HashMap<String, String>, url_arg: Option<&str>,
                     interactive: bool) -> Result<(String, String), String> {
    let mut file_values = HashMap::new();
    let candidates: Vec<&str> = match env_file {
        Some(f) => vec![f],
        None => ENV_FILES.to_vec(),
    };
    for path in candidates {
        if Path::new(path).is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(path).map(|m| m.permissions().mode()).unwrap_or(0);
                if mode & 0o077 != 0 {
                    eprintln!("warning: {path} is readable by other users; run: chmod 600 {path}");
                }
            }
            file_values = parse_env_file(path).map_err(|e| format!("error: cannot read {path}: {e}"))?;
            break;
        }
        if env_file.is_some() {
            return Err(format!("error: env file not found: {path}"));
        }
    }
    let get = |k: &str| {
        environ.get(k).filter(|v| !v.is_empty()).or_else(|| file_values.get(k).filter(|v| !v.is_empty())).cloned()
    };
    let mut token = get("MATRIX_HA_TOKEN");
    if token.is_none() && interactive {
        token = rpassword::prompt_password("Home Assistant long-lived token (hidden): ")
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
    }
    let Some(token) = token else {
        return Err("error: no token. Put MATRIX_HA_TOKEN=... in ./secrets.env (see config/secrets.env.example) \
                    or export MATRIX_HA_TOKEN."
            .into());
    };
    let url = url_arg.map(String::from).or_else(|| get("MATRIX_HA_URL")).unwrap_or_else(|| DEFAULT_HA_URL.into());
    Ok((normalize_url(&url), token))
}

pub fn fetch_states(url: &str, token: &str) -> Result<Vec<Value>, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).build();
    let result = agent
        .get(&format!("{url}/api/states"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .call();
    match result {
        Ok(resp) => match resp.into_json::<Value>() {
            Ok(Value::Array(a)) => Ok(a),
            _ => Err(format!("error: {url}/api/states did not return a list")),
        },
        Err(ureq::Error::Status(401, _)) => Err("error: Home Assistant rejected the token (401). Create a new \
                                                  long-lived token and update secrets.env."
            .into()),
        Err(ureq::Error::Status(code, _)) => {
            Err(format!("error: Home Assistant returned HTTP {code} for {url}/api/states"))
        }
        Err(ureq::Error::Transport(t)) => Err(format!("error: cannot reach {url}: {:?}", t.kind())),
    }
}

fn table(out: &mut dyn Write, title: &str, rows: &[&Entry], note: &str, hidden: usize) -> std::io::Result<()> {
    writeln!(out, "\n{title} ({}){note}", rows.len())?;
    if hidden > 0 {
        writeln!(out, "  ({hidden} more hidden; use --all-lights to show them)")?;
    }
    if rows.is_empty() {
        return writeln!(out, "  (none found)");
    }
    let width = rows.iter().map(|r| r.entity_id.len()).max().unwrap_or(0);
    for r in rows {
        let flags = if r.is_group { " [group]" } else { "" };
        let cls = if r.device_class.is_empty() { String::new() } else { format!(" ({})", r.device_class) };
        writeln!(out, "  {:<width$}  {:<12} {}{cls}{flags}", r.entity_id, r.state, r.name)?;
    }
    Ok(())
}

pub fn toml_snippet(lights: &[Entry], gates: &[Entry]) -> String {
    let list = |items: Vec<&str>| format!("[{}]", items.iter().map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(", "));
    let mut excluded: Vec<&str> = lights.iter().filter(|l| l.is_group).map(|l| l.entity_id.as_str()).collect();
    if lights.iter().any(|l| l.is_noise) {
        excluded.extend(NOISE_GLOBS);
    }
    [
        "[homeassistant]".to_string(),
        "# Review each list; delete entries you do not want counted.".to_string(),
        format!("gates = {}", list(gates.iter().map(|g| g.entity_id.as_str()).collect())),
        "exclude_locks = []".to_string(),
        "# Globs (fnmatch) matched against entity ids; light groups and status LEDs are excluded.".to_string(),
        format!("exclude_lights = {}", list(excluded)),
    ]
    .join("\n")
}

pub struct Options {
    pub all_doors: bool,
    pub all_lights: bool,
    pub toml: bool,
}

pub fn report(out: &mut dyn Write, states: &[Value], opts: &Options) -> std::io::Result<()> {
    let c = classify(states);
    table(out, "Locks", &c.locks.iter().collect::<Vec<_>>(), "", 0)?;
    let real: Vec<&Entry> = c.lights.iter().filter(|l| !l.is_noise).collect();
    let shown: Vec<&Entry> = if opts.all_lights { c.lights.iter().collect() } else { real.clone() };
    let on = real.iter().filter(|l| l.state == "on").count();
    table(
        out,
        "Lights",
        &shown,
        &format!(" - {} room lights, {on} on now; groups flagged [group]", real.len()),
        if opts.all_lights { 0 } else { c.lights.len() - real.len() },
    )?;
    table(out, "Gate candidates", &c.gates.iter().collect::<Vec<_>>(), " - entities whose id or name contains 'gate'", 0)?;
    if opts.all_doors || c.gates.is_empty() {
        table(out, "Other door/garage/opening entities", &c.other.iter().collect::<Vec<_>>(),
              " - pick from here if no gate matched by name", 0)?;
    }
    if opts.toml {
        writeln!(out, "\n# --- paste into config/config.toml ---")?;
        writeln!(out, "{}", toml_snippet(&c.lights, &c.gates))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TOKEN: &str = "eyJfake.token.value-for-tests-only";

    fn states() -> Vec<Value> {
        vec![
            json!({"entity_id": "lock.front_door", "state": "locked", "attributes": {"friendly_name": "Front Door"}}),
            json!({"entity_id": "lock.back_door", "state": "unlocked", "attributes": {"friendly_name": "Back Door"}}),
            json!({"entity_id": "light.kitchen", "state": "on", "attributes": {"friendly_name": "Kitchen"}}),
            json!({"entity_id": "light.downstairs", "state": "on",
                   "attributes": {"friendly_name": "Downstairs", "entity_id": ["light.kitchen"]}}),
            json!({"entity_id": "cover.back_gate", "state": "closed", "attributes": {"friendly_name": "Back Gate"}}),
            json!({"entity_id": "binary_sensor.side_gate_contact", "state": "off",
                   "attributes": {"friendly_name": "Side contact", "device_class": "opening"}}),
            json!({"entity_id": "binary_sensor.garage", "state": "off", "attributes": {"device_class": "garage_door"}}),
            json!({"entity_id": "light.hall_switch_light_rgb_indicator", "state": "on", "attributes": {}}),
            json!({"entity_id": "sensor.temp", "state": "70", "attributes": {}}),
        ]
    }

    fn ids(v: &[Entry]) -> Vec<&str> {
        v.iter().map(|e| e.entity_id.as_str()).collect()
    }

    #[test]
    fn classify_sorts_and_groups() {
        let c = classify(&states());
        assert_eq!(ids(&c.locks), ["lock.back_door", "lock.front_door"]);
        assert!(c.lights.iter().find(|l| l.entity_id == "light.downstairs").unwrap().is_group);
        assert_eq!(ids(&c.gates), ["binary_sensor.side_gate_contact", "cover.back_gate"]);
        assert_eq!(ids(&c.other), ["binary_sensor.garage"]);
    }

    #[test]
    fn noise_lights_flagged_and_excluded_in_toml() {
        let c = classify(&states());
        let noisy: Vec<&str> = c.lights.iter().filter(|l| l.is_noise).map(|l| l.entity_id.as_str()).collect();
        assert_eq!(noisy, ["light.hall_switch_light_rgb_indicator"]);
        let snippet = toml_snippet(&c.lights, &[]);
        assert!(snippet.contains("\"light.*_rgb_indicator\""));
        assert!(snippet.contains("\"light.downstairs\", \"light.*_rgb_indicator\""));
    }

    #[test]
    fn url_normalizing() {
        assert_eq!(normalize_url("homeassistant:8123/"), "http://homeassistant:8123");
    }

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn token_from_env_file_and_url_default() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("s.env");
        std::fs::write(&f, format!("# c\nMATRIX_HA_TOKEN='{TOKEN}'\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let got = load_settings(f.to_str(), &env(&[]), None, false).unwrap();
        assert_eq!(got, (DEFAULT_HA_URL.to_string(), TOKEN.to_string()));
        assert!(load_settings(Some("/nonexistent/x.env"), &env(&[]), None, false).is_err());
    }

    #[test]
    fn env_beats_file_and_missing_token_fails() {
        let got = load_settings(None, &env(&[("MATRIX_HA_TOKEN", "x"), ("MATRIX_HA_URL", "h:1")]), None, false);
        // (a ./secrets.env or /etc/matrix/secrets.env on this machine could only add to this)
        assert_eq!(got.unwrap(), ("http://h:1".to_string(), "x".to_string()));
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.env");
        std::fs::write(&empty, "").unwrap();
        assert!(load_settings(empty.to_str(), &env(&[]), None, false).is_err());
    }

    #[test]
    fn report_never_prints_the_token_and_lists_entities() {
        let mut out = Vec::new();
        report(&mut out, &states(), &Options { all_doors: false, all_lights: false, toml: true }).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("cover.back_gate") && text.contains("[group]"));
        assert!(text.contains("\"light.downstairs\", \"light.*_rgb_indicator\""));
        assert!(text.contains("1 more hidden"));
        assert!(!text.contains(TOKEN));
    }
}
