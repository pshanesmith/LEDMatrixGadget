//! One-time ZIP / postal code geocoding (DESIGN.md section 5). Never used at runtime.
//!
//! Open-Meteo geocoding first, OpenStreetMap Nominatim as fallback. Results are always filtered
//! by country: a bare "10001" also matches places in Spain and France.

use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

pub const OPEN_METEO_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
pub const NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org/search";
/// Usage policy: at most one request per second.
pub const NOMINATIM_MIN_INTERVAL: f64 = 1.0;

/// Identifies the application (Nominatim policy) without naming a person; optionally set
/// MATRIX_CONTACT to append your own contact if you want one.
pub fn user_agent() -> String {
    let mut ua = "LEDMatrixGadget/0.2 (home LED matrix; one-time ZIP lookup)".to_string();
    if let Ok(contact) = std::env::var("MATRIX_CONTACT") {
        if !contact.is_empty() {
            ua.push(' ');
            ua.push_str(&contact);
        }
    }
    ua
}

const US_STATES: [(&str, &str); 52] = [
    ("Alabama", "AL"), ("Alaska", "AK"), ("Arizona", "AZ"), ("Arkansas", "AR"), ("California", "CA"),
    ("Colorado", "CO"), ("Connecticut", "CT"), ("Delaware", "DE"), ("District of Columbia", "DC"),
    ("Florida", "FL"), ("Georgia", "GA"), ("Hawaii", "HI"), ("Idaho", "ID"), ("Illinois", "IL"),
    ("Indiana", "IN"), ("Iowa", "IA"), ("Kansas", "KS"), ("Kentucky", "KY"), ("Louisiana", "LA"),
    ("Maine", "ME"), ("Maryland", "MD"), ("Massachusetts", "MA"), ("Michigan", "MI"),
    ("Minnesota", "MN"), ("Mississippi", "MS"), ("Missouri", "MO"), ("Montana", "MT"),
    ("Nebraska", "NE"), ("Nevada", "NV"), ("New Hampshire", "NH"), ("New Jersey", "NJ"),
    ("New Mexico", "NM"), ("New York", "NY"), ("North Carolina", "NC"), ("North Dakota", "ND"),
    ("Ohio", "OH"), ("Oklahoma", "OK"), ("Oregon", "OR"), ("Pennsylvania", "PA"),
    ("Rhode Island", "RI"), ("South Carolina", "SC"), ("South Dakota", "SD"), ("Tennessee", "TN"),
    ("Texas", "TX"), ("Utah", "UT"), ("Vermont", "VT"), ("Virginia", "VA"), ("Washington", "WA"),
    ("West Virginia", "WV"), ("Wisconsin", "WI"), ("Wyoming", "WY"), ("Puerto Rico", "PR"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct GeocodeError(pub String);

impl fmt::Display for GeocodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GeocodeError {}

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub label: String,
    pub latitude: f64,
    pub longitude: f64,
    pub country_code: String,
    pub provider: String,
}

impl Place {
    pub fn new(label: &str, latitude: f64, longitude: f64, country_code: &str, provider: &str) -> Place {
        Place {
            label: label.into(),
            latitude,
            longitude,
            country_code: country_code.into(),
            provider: provider.into(),
        }
    }

    pub fn rounded(&self) -> (f64, f64) {
        let r = |x: f64| (x * 1000.0).round() / 1000.0;
        (r(self.latitude), r(self.longitude))
    }
}

/// GET a URL and parse the JSON body; the error is the text to report.
pub type Getter<'a> = &'a dyn Fn(&str) -> Result<Value, String>;

pub fn http_get_json(url: &str) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build();
    let resp = agent.get(url).set("User-Agent", &user_agent()).call().map_err(|e| e.to_string())?;
    resp.into_json::<Value>().map_err(|e| e.to_string())
}

/// `urllib.parse.urlencode`: `+` for spaces, percent-escapes for everything but `_.-~` and alphanumerics.
pub fn urlencode(params: &[(&str, &str)]) -> String {
    fn quote(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => out.push(b as char),
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }
    params.iter().map(|(k, v)| format!("{}={}", quote(k), quote(v))).collect::<Vec<_>>().join("&")
}

fn state_label(state: &str, country_code: &str) -> String {
    if country_code.eq_ignore_ascii_case("US") {
        US_STATES.iter().find(|(n, _)| *n == state).map_or(state, |(_, a)| a).to_string()
    } else {
        state.to_string()
    }
}

fn join(parts: &[&str]) -> String {
    parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(", ")
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn float_of(v: &Value, key: &str) -> Result<f64, String> {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_f64().ok_or_else(|| format!("bad {key}")),
        Some(Value::String(s)) => s.trim().parse().map_err(|_| format!("bad {key}")),
        _ => Err(format!("missing {key}")),
    }
}

pub fn search_open_meteo(postal_code: &str, country: &str, get: Getter) -> Result<Vec<Place>, String> {
    let url = format!(
        "{OPEN_METEO_URL}?{}",
        urlencode(&[("name", postal_code), ("count", "10"), ("language", "en"), ("format", "json"),
                    ("countryCode", &country.to_uppercase())])
    );
    let data = get(&url)?;
    let results = match &data {
        Value::Null => Vec::new(),
        Value::Object(o) => o.get("results").and_then(Value::as_array).cloned().unwrap_or_default(),
        _ => return Err("unexpected response".into()),
    };
    let mut places: Vec<(bool, Place)> = Vec::new();
    for r in &results {
        let code = text(r, "country_code").to_uppercase();
        if code != country.to_uppercase() {
            continue; // belt and braces: the API filter is not trusted alone
        }
        // Prefer matches that actually list this postal code.
        let listed = r
            .get("postcodes")
            .and_then(Value::as_array)
            .is_some_and(|p| p.iter().any(|c| c.as_str() == Some(postal_code)));
        let label = join(&[text(r, "name"), &state_label(text(r, "admin1"), &code)]);
        places.push((!listed, Place::new(&label, float_of(r, "latitude")?, float_of(r, "longitude")?, &code, "open-meteo")));
    }
    places.sort_by_key(|(unlisted, _)| *unlisted); // stable
    Ok(places.into_iter().map(|(_, p)| p).collect())
}

/// Spaces calls to Nominatim at least `interval` seconds apart.
pub struct Throttle {
    interval: f64,
    clock: Box<dyn FnMut() -> f64>,
    sleep: Box<dyn FnMut(f64)>,
    last: Option<f64>,
}

impl Throttle {
    pub fn new(interval: f64) -> Throttle {
        let start = Instant::now();
        Throttle::with(
            interval,
            Box::new(move || start.elapsed().as_secs_f64()),
            Box::new(|s| std::thread::sleep(Duration::from_secs_f64(s))),
        )
    }

    pub fn with(interval: f64, clock: Box<dyn FnMut() -> f64>, sleep: Box<dyn FnMut(f64)>) -> Throttle {
        Throttle { interval, clock, sleep, last: None }
    }

    pub fn wait(&mut self) {
        if let Some(last) = self.last {
            let remaining = self.interval - ((self.clock)() - last);
            if remaining > 0.0 {
                (self.sleep)(remaining);
            }
        }
        self.last = Some((self.clock)());
    }
}

pub fn search_nominatim(postal_code: &str, country: &str, get: Getter, throttle: &mut Throttle)
                        -> Result<Vec<Place>, String> {
    let url = format!(
        "{NOMINATIM_URL}?{}",
        urlencode(&[("postalcode", postal_code), ("country", &country.to_uppercase()),
                    ("format", "jsonv2"), ("addressdetails", "1"), ("limit", "5")])
    );
    throttle.wait();
    let data = get(&url)?;
    let items = match &data {
        Value::Null => Vec::new(),
        Value::Array(a) => a.clone(),
        _ => return Err("unexpected response".into()),
    };
    let empty = Value::Object(Default::default());
    let mut places = Vec::new();
    for r in &items {
        let addr = r.get("address").unwrap_or(&empty);
        let code = Some(text(addr, "country_code")).filter(|c| !c.is_empty()).unwrap_or(country).to_uppercase();
        if code != country.to_uppercase() {
            continue;
        }
        let town = ["city", "town", "village", "municipality", "county"]
            .iter()
            .map(|k| text(addr, k))
            .find(|t| !t.is_empty())
            .unwrap_or_else(|| text(r, "name"));
        let label = join(&[town, &state_label(text(addr, "state"), &code)]);
        places.push(Place::new(&label, float_of(r, "lat")?, float_of(r, "lon")?, &code, "nominatim"));
    }
    Ok(places)
}

fn looks_like_postal_code(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=10).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b[1..].iter().all(|c| c.is_ascii_alphanumeric() || *c == b' ' || *c == b'-')
}

/// Matches for a postal code, country-filtered. Errors if nothing is found or every provider
/// failed (for example offline).
pub fn geocode(postal_code: &str, country: &str, get: Getter, throttle: &mut Throttle)
               -> Result<Vec<Place>, GeocodeError> {
    let postal_code = postal_code.trim();
    if !looks_like_postal_code(postal_code) {
        return Err(GeocodeError(format!("{postal_code:?} does not look like a postal code")));
    }
    let mut errors = Vec::new();
    for name in ["open-meteo", "nominatim"] {
        let result = if name == "open-meteo" {
            search_open_meteo(postal_code, country, get)
        } else {
            search_nominatim(postal_code, country, get, throttle)
        };
        match result {
            Err(e) => errors.push(format!("{name}: {e}")),
            Ok(places) if !places.is_empty() => return Ok(places),
            Ok(_) => errors.push(format!("{name}: no match")),
        }
    }
    if errors.iter().all(|e| e.ends_with("no match")) {
        return Err(GeocodeError(format!("no match for {postal_code} in {}", country.to_uppercase())));
    }
    Err(GeocodeError(format!(
        "geocoding failed (offline?): {}. Retry later or set latitude/longitude in config.toml by hand.",
        errors.join("; ")
    )))
}

// --- config.toml update ----------------------------------------------------------------

/// The name in a `[section]` header line (trailing comment allowed), else None.
pub fn section_name(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('[')?;
    let close = rest.find(']')?;
    let (name, after) = (&rest[..close], &rest[close + 1..]);
    let after = after.trim_start();
    (!name.is_empty() && (after.is_empty() || after.starts_with('#'))).then_some(name)
}

/// `(indent, key, " = ", end)` of a `key = ` line.
pub fn key_of(line: &str) -> Option<(&str, &str, &str, usize)> {
    let indent_len = line.len() - line.trim_start().len();
    let rest = &line[indent_len..];
    let key_len = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-').count();
    if key_len == 0 {
        return None;
    }
    let after_key = &rest[key_len..];
    let ws1 = after_key.len() - after_key.trim_start().len();
    let after_ws = &after_key[ws1..];
    let after_eq = after_ws.strip_prefix('=')?;
    let ws2 = after_eq.len() - after_eq.trim_start().len();
    let end = indent_len + key_len + ws1 + 1 + ws2;
    Some((&line[..indent_len], &rest[..key_len], &line[indent_len + key_len..end], end))
}

#[derive(Debug, Clone, PartialEq)]
pub enum TomlVal {
    Str(String),
    Num(f64),
}

impl TomlVal {
    fn render(&self) -> String {
        match self {
            // JSON string escaping is valid TOML basic-string syntax
            TomlVal::Str(s) => serde_json::to_string(s).expect("string serialises"),
            TomlVal::Num(n) => format!("{n:?}"),
        }
    }
}

/// Start of a trailing `   # comment` on the rest of a value line, if any.
fn trailing_comment(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && b[j] == b'#' {
                return Some(&s[i..]);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

/// Set keys in the `[location]` table, leaving every other line (and comment) untouched.
///
/// Existing keys keep their position; new keys are added at the end of the table; a missing table
/// is appended at the end of the file.
pub fn update_location(text: &str, values: &[(&str, TomlVal)]) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let start = lines.iter().position(|l| section_name(l).is_some_and(|n| n.trim() == "location"));
    let Some(start) = start else {
        let mut out = lines.clone();
        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push("[location]".into());
        out.extend(values.iter().map(|(k, v)| format!("{k} = {}", v.render())));
        return out.join("\n") + "\n";
    };
    let end = (start + 1..lines.len()).find(|&i| section_name(&lines[i]).is_some()).unwrap_or(lines.len());
    let mut pending: Vec<(&str, &TomlVal)> = values.iter().map(|(k, v)| (*k, v)).collect();
    for line in &mut lines[start + 1..end] {
        let Some((indent, key, eq, key_end)) = key_of(line) else { continue };
        if let Some(pos) = pending.iter().position(|(k, _)| *k == key) {
            let (_, val) = pending.remove(pos);
            let comment = trailing_comment(&line[key_end..]).unwrap_or("");
            *line = format!("{indent}{key}{eq}{}{comment}", val.render());
        }
    }
    let mut insert_at = end;
    while insert_at > start + 1 && lines[insert_at - 1].trim().is_empty() {
        insert_at -= 1; // keep blank lines/comments for the next section below
    }
    while insert_at > start + 1 && lines[insert_at - 1].trim_start().starts_with('#') && insert_at == end {
        insert_at -= 1; // a trailing comment belongs to the following section
    }
    let new: Vec<String> = pending.iter().map(|(k, v)| format!("{k} = {}", v.render())).collect();
    lines.splice(insert_at..insert_at, new);
    lines.join("\n") + "\n"
}

pub fn write_location(path: &Path, place: &Place, postal_code: &str, units: Option<&str>) -> Result<(), String> {
    let text = if path.exists() { std::fs::read_to_string(path).map_err(|e| e.to_string())? } else { String::new() };
    let (lat, lon) = place.rounded();
    let mut values = vec![
        ("postal_code", TomlVal::Str(postal_code.into())),
        ("latitude", TomlVal::Num(lat)),
        ("longitude", TomlVal::Num(lon)),
        ("label", TomlVal::Str(place.label.clone())),
    ];
    let has_units = text.lines().any(|l| key_of(l).is_some_and(|(_, k, _, _)| k == "units"));
    if let Some(u) = units {
        values.push(("units", TomlVal::Str(u.into())));
    } else if !has_units {
        values.push(("units", TomlVal::Str("fahrenheit".into())));
    }
    let new = update_location(&text, &values);
    new.parse::<toml::Table>().map_err(|e| format!("refusing to write a file that does not parse: {e}"))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, &new).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn fixture(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// A fake transport: the first entry whose key is in the URL answers.
    struct Fake {
        responses: Vec<(&'static str, Result<Value, String>)>,
        calls: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(responses: Vec<(&'static str, Result<Value, String>)>) -> Fake {
            Fake { responses, calls: RefCell::new(Vec::new()) }
        }

        fn get(&self, url: &str) -> Result<Value, String> {
            self.calls.borrow_mut().push(url.to_string());
            self.responses
                .iter()
                .find(|(k, _)| url.contains(k))
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("unexpected {url}"))
        }
    }

    fn no_wait() -> Throttle {
        Throttle::with(0.0, Box::new(|| 0.0), Box::new(|_| {}))
    }

    #[test]
    fn open_meteo_filters_other_countries() {
        // the unfiltered recorded response also contains Spanish and French matches
        let f = Fake::new(vec![("geocoding-api", Ok(fixture("geocode_openmeteo_10001.json")))]);
        let places = search_open_meteo("10001", "US", &|u| f.get(u)).unwrap();
        assert_eq!(places.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["New York, NY"]);
        assert!(f.calls.borrow()[0].contains("countryCode=US"));
    }

    #[test]
    fn open_meteo_other_country() {
        let f = Fake::new(vec![("geocoding-api", Ok(fixture("geocode_openmeteo_10001.json")))]);
        let places = search_open_meteo("10001", "ES", &|u| f.get(u)).unwrap();
        assert_eq!(places[0].country_code, "ES");
    }

    #[test]
    fn nominatim_structured_query_and_label() {
        let f = Fake::new(vec![("nominatim", Ok(fixture("geocode_nominatim_10001_us.json")))]);
        let places = search_nominatim("10001", "US", &|u| f.get(u), &mut no_wait()).unwrap();
        assert_eq!(places[0].label, "New York, NY");
        assert_eq!(places[0].rounded(), (40.748, -73.994));
        let url = f.calls.borrow()[0].clone();
        assert!(url.contains("postalcode=10001") && url.contains("country=US"));
    }

    #[test]
    fn geocode_prefers_open_meteo() {
        let f = Fake::new(vec![("geocoding-api", Ok(fixture("geocode_openmeteo_10001_us.json")))]);
        let places = geocode("10001", "US", &|u| f.get(u), &mut no_wait()).unwrap();
        assert_eq!(places[0].provider, "open-meteo");
        assert_eq!(f.calls.borrow().len(), 1);
    }

    #[test]
    fn geocode_falls_back_to_nominatim() {
        let firsts = [Ok(serde_json::json!({"results": []})), Err("down".to_string()), Ok(serde_json::json!({}))];
        for first in firsts {
            let f = Fake::new(vec![
                ("geocoding-api", first),
                ("nominatim", Ok(fixture("geocode_nominatim_10001_us.json"))),
            ]);
            assert_eq!(geocode("10001", "US", &|u| f.get(u), &mut no_wait()).unwrap()[0].provider, "nominatim");
        }
    }

    #[test]
    fn offline_failure() {
        let f = Fake::new(vec![("geocoding-api", Err("offline".into())), ("nominatim", Err("offline".into()))]);
        let e = geocode("10001", "US", &|u| f.get(u), &mut no_wait()).unwrap_err();
        assert!(e.0.contains("offline"), "{e}");
    }

    #[test]
    fn no_match_anywhere() {
        let f = Fake::new(vec![("geocoding-api", Ok(serde_json::json!({}))), ("nominatim", Ok(serde_json::json!([])))]);
        let e = geocode("00000", "US", &|u| f.get(u), &mut no_wait()).unwrap_err();
        assert!(e.0.contains("no match"), "{e}");
    }

    #[test]
    fn rejects_junk_postal_code() {
        let f = Fake::new(vec![]);
        assert!(geocode("10001&evil=1", "US", &|u| f.get(u), &mut no_wait()).is_err());
        assert!(geocode("1", "US", &|u| f.get(u), &mut no_wait()).is_err());
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn nominatim_throttle_waits_one_second() {
        use std::rc::Rc;
        let now = Rc::new(RefCell::new(100.0));
        let slept = Rc::new(RefCell::new(Vec::new()));
        let (n1, n2, s) = (Rc::clone(&now), Rc::clone(&now), Rc::clone(&slept));
        let mut t = Throttle::with(
            1.0,
            Box::new(move || *n1.borrow()),
            Box::new(move |secs| {
                s.borrow_mut().push(secs);
                *n2.borrow_mut() += secs;
            }),
        );
        t.wait();
        *now.borrow_mut() += 0.25;
        t.wait();
        assert_eq!(*slept.borrow(), [0.75]);
    }

    #[test]
    fn urlencode_matches_python() {
        assert_eq!(urlencode(&[("a b", "c&d"), ("k", "é")]), "a+b=c%26d&k=%C3%A9");
    }

    const CONFIG: &str = "\
# Local config
[location]
postal_code = \"00000\"   # old
latitude = 1.0
longitude = 2.0
units = \"celsius\"

# Home Assistant
[homeassistant]
url = \"http://homeassistant:8123\"
";

    fn place() -> Place {
        Place::new("New York, NY", 40.74848, -73.99389, "US", "test")
    }

    fn parsed(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    #[test]
    fn update_replaces_keys_and_keeps_everything_else() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, CONFIG).unwrap();
        write_location(&p, &place(), "10001", None).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# Local config") && text.contains("# Home Assistant"));
        assert!(text.contains("url = \"http://homeassistant:8123\"") && text.contains("# old"));
        let loc = &parsed(&text)["location"];
        assert_eq!(loc["postal_code"].as_str(), Some("10001"));
        assert_eq!(loc["latitude"].as_float(), Some(40.748));
        assert_eq!(loc["longitude"].as_float(), Some(-73.994));
        assert_eq!(loc["label"].as_str(), Some("New York, NY"));
        assert_eq!(loc["units"].as_str(), Some("celsius"));
    }

    #[test]
    fn update_adds_missing_table_and_keys() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[homeassistant]\nurl = \"x\"\n").unwrap();
        write_location(&p, &place(), "10001", Some("fahrenheit")).unwrap();
        let data = parsed(&std::fs::read_to_string(&p).unwrap());
        assert_eq!(data["location"]["units"].as_str(), Some("fahrenheit"));
        assert_eq!(data["homeassistant"]["url"].as_str(), Some("x"));
        let p2 = dir.path().join("new.toml");
        write_location(&p2, &place(), "10001", None).unwrap();
        assert_eq!(parsed(&std::fs::read_to_string(&p2).unwrap())["location"]["units"].as_str(), Some("fahrenheit"));
    }

    #[test]
    fn written_config_loads_with_the_project_loader() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, CONFIG).unwrap();
        write_location(&p, &place(), "10001", None).unwrap();
        let cfg = crate::config::load_config_with(p.to_str(), &Default::default()).unwrap();
        let loc = cfg.require_location().unwrap();
        assert_eq!((loc.latitude, loc.longitude, loc.postal_code.as_str()), (40.748, -73.994, "10001"));
    }

    #[test]
    fn label_with_quotes_is_escaped() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        write_location(&p, &Place::new("Say \"hi\", NC", 1.0, 2.0, "US", "t"), "12345", None).unwrap();
        let data = parsed(&std::fs::read_to_string(&p).unwrap());
        assert_eq!(data["location"]["label"].as_str(), Some("Say \"hi\", NC"));
    }

    #[test]
    fn header_and_key_parsing() {
        assert_eq!(section_name("  [location]  # c"), Some("location"));
        assert_eq!(section_name("[[x]]"), None);
        assert_eq!(section_name("a = [1]"), None);
        assert_eq!(key_of("  units = \"c\"").map(|k| k.1), Some("units"));
        assert_eq!(key_of("# x = 1"), None);
    }
}
