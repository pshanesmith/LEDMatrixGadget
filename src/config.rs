//! Configuration: config.toml plus environment overrides (secrets never live in the file).

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;

pub const DEFAULT_HA_URL: &str = "http://homeassistant:8123";
pub const CONFIG_SEARCH: [&str; 2] = ["config/config.toml", "/etc/matrix/config.toml"];
pub const SECRETS_SEARCH: [&str; 2] = ["secrets.env", "/etc/matrix/secrets.env"];

pub type Env = HashMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

fn err<T>(msg: impl Into<String>) -> Result<T, ConfigError> {
    Err(ConfigError(msg.into()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayConfig {
    pub rows: u32,
    pub cols: u32,
    pub chain_length: u32,
    pub parallel: u32,
    pub hardware_mapping: String,
    pub gpio_slowdown: u32,
    pub pwm_bits: u8,
    pub pwm_lsb_nanoseconds: u32,
    /// 0 = unlimited; a steady cap can remove flicker caused by rate swings.
    pub limit_refresh_hz: u32,
    pub row_address_type: u32,
    pub multiplexing: u32,
    pub rgb_sequence: String,
    pub brightness: u8,
    pub max_brightness: u8,
    pub scene_fps: u32,
    /// The panel library drops root after GPIO setup; env MATRIX_DROP_PRIVILEGES=0 disables.
    pub drop_privileges: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        DisplayConfig {
            rows: 64,
            cols: 64,
            chain_length: 1,
            parallel: 1,
            hardware_mapping: "adafruit-hat-pwm".into(),
            gpio_slowdown: 2,
            pwm_bits: 11,
            pwm_lsb_nanoseconds: 130,
            limit_refresh_hz: 0,
            row_address_type: 0,
            multiplexing: 0,
            rgb_sequence: "RGB".into(),
            brightness: 50,
            max_brightness: 70,
            scene_fps: 15,
            drop_privileges: true,
        }
    }
}

impl DisplayConfig {
    pub fn width(&self) -> usize {
        (self.cols * self.chain_length) as usize
    }

    pub fn height(&self) -> usize {
        (self.rows * self.parallel) as usize
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocationConfig {
    pub latitude: f64,
    pub longitude: f64,
    pub postal_code: String,
    pub label: String,
    pub units: String,
}

impl LocationConfig {
    pub fn new(latitude: f64, longitude: f64) -> Self {
        LocationConfig {
            latitude,
            longitude,
            postal_code: String::new(),
            label: String::new(),
            units: "fahrenheit".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeatherConfig {
    pub refresh_seconds: u64,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        WeatherConfig { refresh_seconds: 900 }
    }
}

#[derive(Clone, PartialEq)]
pub struct HomeAssistantConfig {
    pub url: String,
    /// From the environment only; never printed.
    pub token: String,
    pub poll_seconds: u64,
    pub gates: Vec<String>,
    /// fnmatch globs on entity ids.
    pub exclude_locks: Vec<String>,
    pub exclude_lights: Vec<String>,
    /// Sensor with the rate tier; empty hides the bolt.
    pub power_tier_entity: String,
    /// fnmatch globs on update entity ids to ignore.
    pub exclude_updates: Vec<String>,
}

impl Default for HomeAssistantConfig {
    fn default() -> Self {
        HomeAssistantConfig {
            url: DEFAULT_HA_URL.into(),
            token: String::new(),
            poll_seconds: 15,
            gates: Vec::new(),
            exclude_locks: Vec::new(),
            exclude_lights: Vec::new(),
            power_tier_entity: String::new(),
            exclude_updates: Vec::new(),
        }
    }
}

impl fmt::Debug for HomeAssistantConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HomeAssistantConfig")
            .field("url", &self.url)
            .field("token", &if self.token.is_empty() { "" } else { "<redacted>" })
            .field("poll_seconds", &self.poll_seconds)
            .field("gates", &self.gates)
            .field("exclude_locks", &self.exclude_locks)
            .field("exclude_lights", &self.exclude_lights)
            .field("power_tier_entity", &self.power_tier_entity)
            .field("exclude_updates", &self.exclude_updates)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    pub display: DisplayConfig,
    pub location: Option<LocationConfig>,
    pub weather: WeatherConfig,
    pub homeassistant: HomeAssistantConfig,
}

impl Config {
    pub fn require_location(&self) -> Result<&LocationConfig, ConfigError> {
        self.location.as_ref().ok_or_else(|| {
            ConfigError(
                "[location] is not set; run matrix-geocode <ZIP> or add latitude/longitude to config.toml"
                    .into(),
            )
        })
    }
}

/// Accept 'host', 'host:port' or a full URL; default scheme http, no trailing slash.
pub fn normalize_url(url: &str) -> Result<String, ConfigError> {
    let mut url = url.trim().to_string();
    if url.is_empty() {
        return err("homeassistant url is empty");
    }
    if !url.contains("://") {
        url = format!("http://{url}");
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return err(format!("homeassistant url must be http(s): {url:?}"));
    }
    Ok(url.trim_end_matches('/').to_string())
}

/// `KEY=value` lines; blank lines and `#` comments are skipped, surrounding quotes removed.
pub fn parse_env_text(text: &str) -> Env {
    let mut values = Env::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            values.insert(
                key.trim().to_string(),
                value.trim().trim_matches(|c| c == '\'' || c == '"').to_string(),
            );
        }
    }
    values
}

pub fn parse_env_file(path: impl AsRef<Path>) -> std::io::Result<Env> {
    Ok(parse_env_text(&fs::read_to_string(path)?))
}

/// One `[section]` of the parsed TOML, with typed getters that report which key was wrong.
struct Section<'a> {
    name: &'a str,
    table: &'a toml::Table,
}

impl<'a> Section<'a> {
    fn new(name: &'a str, value: Option<&'a toml::Value>, empty: &'a toml::Table,
           allowed: &[&str]) -> Result<Self, ConfigError> {
        let table = match value {
            None => empty,
            Some(toml::Value::Table(t)) => t,
            Some(_) => return err(format!("[{name}] must be a table")),
        };
        let mut unknown: Vec<&str> =
            table.keys().map(String::as_str).filter(|k| !allowed.contains(k)).collect();
        if !unknown.is_empty() {
            unknown.sort_unstable();
            return err(format!("unknown key(s) in [{name}]: {}", unknown.join(", ")));
        }
        Ok(Section { name, table })
    }

    fn bad<T>(&self, key: &str, want: &str) -> Result<T, ConfigError> {
        err(format!("[{}] {key} must be {want}", self.name))
    }

    fn int(&self, key: &str, default: i64) -> Result<i64, ConfigError> {
        match self.table.get(key) {
            None => Ok(default),
            Some(toml::Value::Integer(i)) => Ok(*i),
            Some(_) => self.bad(key, "an integer"),
        }
    }

    fn uint<T: TryFrom<i64>>(&self, key: &str, default: i64) -> Result<T, ConfigError> {
        let v = self.int(key, default)?;
        T::try_from(v).or_else(|_| self.bad(key, "a non-negative integer in range"))
    }

    fn float(&self, key: &str) -> Result<Option<f64>, ConfigError> {
        match self.table.get(key) {
            None => Ok(None),
            Some(toml::Value::Float(f)) => Ok(Some(*f)),
            Some(toml::Value::Integer(i)) => Ok(Some(*i as f64)),
            Some(_) => self.bad(key, "a number"),
        }
    }

    fn string(&self, key: &str, default: &str) -> Result<String, ConfigError> {
        match self.table.get(key) {
            None => Ok(default.to_string()),
            Some(toml::Value::String(s)) => Ok(s.clone()),
            Some(_) => self.bad(key, "a string"),
        }
    }

    fn boolean(&self, key: &str, default: bool) -> Result<bool, ConfigError> {
        match self.table.get(key) {
            None => Ok(default),
            Some(toml::Value::Boolean(b)) => Ok(*b),
            Some(_) => self.bad(key, "true or false"),
        }
    }

    fn strings(&self, key: &str) -> Result<Vec<String>, ConfigError> {
        match self.table.get(key) {
            None => Ok(Vec::new()),
            Some(toml::Value::Array(items)) => items
                .iter()
                .map(|v| match v {
                    toml::Value::String(s) => Ok(s.clone()),
                    _ => self.bad(key, "a list of strings"),
                })
                .collect(),
            Some(_) => self.bad(key, "a list of strings"),
        }
    }
}

const DISPLAY_KEYS: [&str; 17] = [
    "rows", "cols", "chain_length", "parallel", "hardware_mapping", "gpio_slowdown", "pwm_bits",
    "pwm_lsb_nanoseconds", "limit_refresh_hz", "row_address_type", "multiplexing", "rgb_sequence",
    "brightness", "max_brightness", "scene_fps",
    // Left over from the Python version (it chose Pillow's image copy path); accepted and ignored.
    "safe_image_copy", "drop_privileges",
];
const LOCATION_KEYS: [&str; 5] = ["latitude", "longitude", "postal_code", "label", "units"];
const WEATHER_KEYS: [&str; 1] = ["refresh_seconds"];
const HA_KEYS: [&str; 8] = [
    "url", "token", "poll_seconds", "gates", "exclude_locks", "exclude_lights",
    "power_tier_entity", "exclude_updates",
];

/// Build a `Config` from parsed TOML. Environment beats secrets file beats TOML.
pub fn build_config(data: &toml::Table, environ: &Env, secrets: &Env) -> Result<Config, ConfigError> {
    let pick = |key: &str| -> Option<String> {
        environ
            .get(key)
            .filter(|v| !v.is_empty())
            .or_else(|| secrets.get(key).filter(|v| !v.is_empty()))
            .cloned()
    };
    let empty = toml::Table::new();

    let s = Section::new("display", data.get("display"), &empty, &DISPLAY_KEYS)?;
    let defaults = DisplayConfig::default();
    let mut d = DisplayConfig {
        rows: s.uint("rows", defaults.rows.into())?,
        cols: s.uint("cols", defaults.cols.into())?,
        chain_length: s.uint("chain_length", defaults.chain_length.into())?,
        parallel: s.uint("parallel", defaults.parallel.into())?,
        hardware_mapping: s.string("hardware_mapping", &defaults.hardware_mapping)?,
        gpio_slowdown: s.uint("gpio_slowdown", defaults.gpio_slowdown.into())?,
        pwm_bits: s.uint("pwm_bits", defaults.pwm_bits.into())?,
        pwm_lsb_nanoseconds: s.uint("pwm_lsb_nanoseconds", defaults.pwm_lsb_nanoseconds.into())?,
        limit_refresh_hz: s.uint("limit_refresh_hz", defaults.limit_refresh_hz.into())?,
        row_address_type: s.uint("row_address_type", defaults.row_address_type.into())?,
        multiplexing: s.uint("multiplexing", defaults.multiplexing.into())?,
        rgb_sequence: s.string("rgb_sequence", &defaults.rgb_sequence)?,
        brightness: 0,
        max_brightness: 0,
        scene_fps: s.uint("scene_fps", defaults.scene_fps.into())?,
        drop_privileges: s.boolean("drop_privileges", defaults.drop_privileges)?,
    };
    let (brightness, max_brightness) =
        (s.int("brightness", defaults.brightness.into())?, s.int("max_brightness", defaults.max_brightness.into())?);
    if !(1..=100).contains(&brightness) || !(1..=100).contains(&max_brightness) {
        return err("brightness values must be 1-100");
    }
    d.brightness = brightness.min(max_brightness) as u8;
    d.max_brightness = max_brightness as u8;
    if let Some(v) = pick("MATRIX_DROP_PRIVILEGES") {
        d.drop_privileges = !matches!(v.trim().to_lowercase().as_str(), "0" | "false" | "no" | "off");
    }

    let location = match data.get("location") {
        None => None,
        Some(v) => {
            let s = Section::new("location", Some(v), &empty, &LOCATION_KEYS)?;
            let (Some(latitude), Some(longitude)) = (s.float("latitude")?, s.float("longitude")?) else {
                return err("[location] needs latitude and longitude");
            };
            let loc = LocationConfig {
                latitude,
                longitude,
                postal_code: s.string("postal_code", "")?,
                label: s.string("label", "")?,
                units: s.string("units", "fahrenheit")?,
            };
            if !((-90.0..=90.0).contains(&loc.latitude) && (-180.0..=180.0).contains(&loc.longitude)) {
                return err("[location] latitude/longitude out of range");
            }
            if loc.units != "fahrenheit" && loc.units != "celsius" {
                return err("[location] units must be 'fahrenheit' or 'celsius'");
            }
            Some(loc)
        }
    };

    let s = Section::new("weather", data.get("weather"), &empty, &WEATHER_KEYS)?;
    let weather = WeatherConfig { refresh_seconds: s.uint("refresh_seconds", 900)? };

    let s = Section::new("homeassistant", data.get("homeassistant"), &empty, &HA_KEYS)?;
    let url_in = pick("MATRIX_HA_URL").map(Ok).unwrap_or_else(|| s.string("url", DEFAULT_HA_URL))?;
    let homeassistant = HomeAssistantConfig {
        url: normalize_url(&url_in)?,
        // Tokens are never read from config.toml.
        token: pick("MATRIX_HA_TOKEN").unwrap_or_default(),
        poll_seconds: s.uint("poll_seconds", 15)?,
        gates: s.strings("gates")?,
        exclude_locks: s.strings("exclude_locks")?,
        exclude_lights: s.strings("exclude_lights")?,
        power_tier_entity: s.string("power_tier_entity", "")?,
        exclude_updates: s.strings("exclude_updates")?,
    };

    Ok(Config { display: d, location, weather, homeassistant })
}

/// Load config.toml (explicit path, else the first existing search path) and secrets.env.
pub fn load_config(path: Option<&str>) -> Result<Config, ConfigError> {
    let environ: Env = std::env::vars().collect();
    load_config_with(path, &environ)
}

pub fn load_config_with(path: Option<&str>, environ: &Env) -> Result<Config, ConfigError> {
    let candidates: Vec<&str> = match path {
        Some(p) => vec![p],
        None => CONFIG_SEARCH.to_vec(),
    };
    let mut data = toml::Table::new();
    match candidates.iter().find(|c| Path::new(c).is_file()) {
        Some(found) => {
            let text = fs::read_to_string(found)
                .map_err(|e| ConfigError(format!("cannot read {found}: {e}")))?;
            data = text.parse::<toml::Table>().map_err(|e| ConfigError(format!("{found}: {e}")))?;
        }
        None => {
            if let Some(p) = path {
                return err(format!("config file not found: {p}"));
            }
        }
    }
    let mut secrets = Env::new();
    if let Some(found) = SECRETS_SEARCH.iter().find(|c| Path::new(c).is_file()) {
        match parse_env_file(found) {
            Ok(values) => secrets = values,
            // After the panel driver drops root, a root-only secrets.env cannot be read again;
            // the service's EnvironmentFile already put the token in the environment.
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                log::debug!("not allowed to read {found}; using the environment only");
            }
            Err(e) => return err(format!("cannot read {found}: {e}")),
        }
    }
    build_config(&data, environ, &secrets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toml(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    fn env(pairs: &[(&str, &str)]) -> Env {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn build(text: &str, pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        build_config(&toml(text), &env(pairs), &Env::new())
    }

    #[test]
    fn normalize_url_forms() {
        assert_eq!(normalize_url("homeassistant:8123/").unwrap(), "http://homeassistant:8123");
        assert_eq!(normalize_url("https://ha.example/").unwrap(), "https://ha.example");
        assert!(normalize_url("ftp://x").is_err());
        assert!(normalize_url("  ").is_err());
    }

    #[test]
    fn defaults_and_ha_url_precedence() {
        let cfg = build("", &[]).unwrap();
        assert_eq!(cfg.homeassistant.url, "http://homeassistant:8123");
        assert_eq!(cfg.display.hardware_mapping, "adafruit-hat-pwm");
        let toml_url = "[homeassistant]\nurl = \"10.0.0.5:8123\"";
        assert_eq!(build(toml_url, &[]).unwrap().homeassistant.url, "http://10.0.0.5:8123");
        let cfg = build(toml_url, &[("MATRIX_HA_URL", "ha.lan")]).unwrap();
        assert_eq!(cfg.homeassistant.url, "http://ha.lan");
    }

    #[test]
    fn token_only_from_environment_and_never_printed() {
        let text = "[homeassistant]\ntoken = \"from-file\"";
        let cfg = build(text, &[("MATRIX_HA_TOKEN", "secret-xyz")]).unwrap();
        assert_eq!(cfg.homeassistant.token, "secret-xyz");
        assert!(!format!("{cfg:?}").contains("secret-xyz"));
        assert_eq!(build(text, &[]).unwrap().homeassistant.token, "");
    }

    #[test]
    fn secrets_file_fills_in_below_the_environment() {
        let secrets = env(&[("MATRIX_HA_TOKEN", "from-secrets")]);
        let cfg = build_config(&toml(""), &Env::new(), &secrets).unwrap();
        assert_eq!(cfg.homeassistant.token, "from-secrets");
        let cfg = build_config(&toml(""), &env(&[("MATRIX_HA_TOKEN", "from-env")]), &secrets).unwrap();
        assert_eq!(cfg.homeassistant.token, "from-env");
    }

    #[test]
    fn validation() {
        assert!(build("[display]\nbogus = 1", &[]).is_err());
        assert!(build("[location]\nlatitude = 95\nlongitude = 0", &[]).is_err());
        assert!(build("[location]\nlatitude = 1.0", &[]).is_err());
        assert!(build("[location]\nlatitude = 1.0\nlongitude = 2.0\nunits = \"kelvin\"", &[]).is_err());
        assert!(build("", &[]).unwrap().require_location().is_err());
        let cfg = build("[display]\nbrightness = 90\nmax_brightness = 70", &[]).unwrap();
        assert_eq!(cfg.display.brightness, 70);
        assert!(build("[display]\nbrightness = 0", &[]).is_err());
    }

    #[test]
    fn old_python_keys_are_still_accepted() {
        assert!(build("[display]\nsafe_image_copy = true", &[]).is_ok());
    }

    #[test]
    fn drop_privileges_default_and_env_override() {
        assert!(build("", &[]).unwrap().display.drop_privileges);
        assert!(!build("", &[("MATRIX_DROP_PRIVILEGES", "0")]).unwrap().display.drop_privileges);
        assert!(!build("", &[("MATRIX_DROP_PRIVILEGES", "no")]).unwrap().display.drop_privileges);
        assert!(build("", &[("MATRIX_DROP_PRIVILEGES", "1")]).unwrap().display.drop_privileges);
        assert!(!build("[display]\ndrop_privileges = false", &[]).unwrap().display.drop_privileges);
    }

    #[test]
    fn env_file_parsing() {
        let v = parse_env_text("# c\nA=1\n B = 'two' \nbad line\nC=\"x=y\"\n");
        assert_eq!(v["A"], "1");
        assert_eq!(v["B"], "two");
        assert_eq!(v["C"], "x=y");
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn the_example_config_loads() {
        let cfg = load_config_with(Some("config/config.example.toml"), &Env::new()).unwrap();
        assert_eq!(cfg.display.pwm_bits, 11);
        assert!(cfg.location.is_some());
        assert_eq!(cfg.homeassistant.exclude_lights.len(), 3);
    }
}
