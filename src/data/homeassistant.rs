//! Home Assistant poller (DESIGN.md section 5, "Home Assistant").
//!
//! One `POST {url}/api/template` per poll. The template runs on the HA side and returns only
//! compact JSON: locks that are not `locked`, lights that are `on` (group lights already dropped
//! via their `entity_id` attribute) and the states of the configured gates. Glob exclusions
//! (fnmatch) are applied here, which keeps the template free of user-supplied patterns.
//! `parse_template_result` turns that JSON into a `HomeSnapshot`; `HomeAssistantPoller` runs it
//! on a thread, keeps the last good snapshot (also on disk) and backs off exponentially after
//! failures.
//!
//! The token is only ever used to build the Authorization header; it is never logged, printed,
//! or put in an error message.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};

use super::{iso, parse_iso, write_atomic};
use crate::config::{normalize_url, HomeAssistantConfig};
use crate::state::{Dt, HomeSnapshot};
use crate::util::{fnmatch, join_within, Signal};

pub const TIMEOUT_SECONDS: u64 = 5;
pub const MAX_BACKOFF_SECONDS: f64 = 60.0;
pub const AUTH_RETRY_SECONDS: f64 = 300.0;
const LOCK_ATTENTION: [&str; 4] = ["unlocked", "jammed", "unlocking", "locking"];
const UNAVAILABLE: [&str; 2] = ["unavailable", "unknown"];
const GATE_OPEN: [&str; 3] = ["on", "open", "opening"];

/// Gate ids are validated before being rendered with a JSON encoder, so nothing from the config
/// can break out of the Jinja string literals.
const TEMPLATE: &str = "\
{%- set tier_entity = __TIER__ -%}
{%- set ns = namespace(locks=[], lights=[]) -%}
{%- set updates = states.update | selectattr('state', 'eq', 'on') | map(attribute='entity_id') | list -%}
{%- for s in states.lock if s.state != 'locked' -%}
{%- set ns.locks = ns.locks + [[s.entity_id, s.state]] -%}
{%- endfor -%}
{%- for s in states.light if s.state == 'on' and s.attributes.get('entity_id') is none -%}
{%- set ns.lights = ns.lights + [s.entity_id] -%}
{%- endfor -%}
{{ {\"locks\": ns.locks, \"lights\": ns.lights, \"updates\": updates,
    \"gates\": __GATES__ | map('states') | list,
    \"tier\": states(tier_entity) if tier_entity else none} | tojson }}";

/// Errors never contain the token.
#[derive(Debug, Clone, PartialEq)]
pub enum HaError {
    Auth(String),
    Unreachable(String),
    BadResponse(String),
}

impl fmt::Display for HaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HaError::Auth(m) | HaError::Unreachable(m) | HaError::BadResponse(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for HaError {}

fn valid_entity_id(id: &str) -> bool {
    let ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    matches!(id.split_once('.'), Some((a, b)) if ok(a) && ok(b))
}

pub fn build_template(gates: &[String], tier_entity: &str) -> Result<String, String> {
    for entity in gates.iter().map(String::as_str).chain((!tier_entity.is_empty()).then_some(tier_entity)) {
        if !valid_entity_id(entity) {
            return Err(format!("invalid entity id: {entity:?}"));
        }
    }
    let gates_json = serde_json::to_string(gates).expect("strings serialise");
    let tier_json = serde_json::to_string(tier_entity).expect("string serialises");
    Ok(TEMPLATE.replace("__GATES__", &gates_json).replace("__TIER__", &tier_json))
}

fn excluded(entity_id: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| fnmatch(entity_id, p))
}

fn shape_error() -> HaError {
    HaError::BadResponse("unexpected template result".into())
}

fn strings(v: &Value) -> Result<Vec<&str>, HaError> {
    v.as_array()
        .ok_or_else(shape_error)?
        .iter()
        .map(|s| s.as_str().ok_or_else(shape_error))
        .collect()
}

/// Turn the rendered template JSON into a `HomeSnapshot`, applying the glob exclusions.
pub fn parse_template_result(data: &Value, config: &HomeAssistantConfig, now: Option<Dt>)
                             -> Result<HomeSnapshot, HaError> {
    let obj = data.as_object().ok_or_else(shape_error)?;
    let locks = obj.get("locks").and_then(Value::as_array).ok_or_else(shape_error)?;
    let lights = strings(obj.get("lights").ok_or_else(shape_error)?)?;
    let states = strings(obj.get("gates").ok_or_else(shape_error)?)?;
    if states.len() != config.gates.len() {
        return Err(shape_error());
    }
    let (mut locks_unlocked, mut locks_unavailable) = (0, 0);
    for lock in locks {
        let pair = lock.as_array().filter(|p| p.len() == 2).ok_or_else(shape_error)?;
        let (entity_id, state) = (
            pair[0].as_str().ok_or_else(shape_error)?,
            pair[1].as_str().ok_or_else(shape_error)?,
        );
        if excluded(entity_id, &config.exclude_locks) {
            continue;
        }
        if LOCK_ATTENTION.contains(&state) {
            locks_unlocked += 1;
        } else if UNAVAILABLE.contains(&state) {
            locks_unavailable += 1;
        }
    }
    let lights_on = lights.iter().filter(|e| !excluded(e, &config.exclude_lights)).count() as u32;
    let gate_ids_where = |wanted: &[&str]| -> Vec<String> {
        config.gates.iter().zip(&states).filter(|(_, s)| wanted.contains(s)).map(|(g, _)| g.clone()).collect()
    };
    let updates_pending = match obj.get("updates") {
        None | Some(Value::Null) => 0,
        Some(v) => strings(v)?.iter().filter(|e| !excluded(e, &config.exclude_updates)).count() as u32,
    };
    let power_tier = (!config.power_tier_entity.is_empty()).then(|| match obj.get("tier") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        None | Some(Value::Null) | Some(Value::String(_)) => "unknown".to_string(),
        Some(other) => other.to_string(),
    });
    Ok(HomeSnapshot {
        fetched_at: now.unwrap_or_else(|| Utc::now().fixed_offset()),
        locks_unlocked,
        locks_unavailable,
        lights_on,
        gates_open: gate_ids_where(&GATE_OPEN),
        gates_unavailable: gate_ids_where(&UNAVAILABLE),
        power_tier,
        updates_pending,
    })
}

pub fn snapshot_to_json(s: &HomeSnapshot) -> Value {
    json!({
        "fetched_at": iso(&s.fetched_at),
        "locks_unlocked": s.locks_unlocked,
        "locks_unavailable": s.locks_unavailable,
        "lights_on": s.lights_on,
        "gates_open": s.gates_open,
        "gates_unavailable": s.gates_unavailable,
        "power_tier": s.power_tier,
        "updates_pending": s.updates_pending,
    })
}

pub fn snapshot_from_json(data: &Value) -> Result<HomeSnapshot, String> {
    let int = |k: &str| data.get(k).and_then(Value::as_u64).map(|v| v as u32).ok_or(format!("bad {k}"));
    let list = |k: &str| -> Result<Vec<String>, String> {
        data.get(k)
            .and_then(Value::as_array)
            .ok_or(format!("bad {k}"))?
            .iter()
            .map(|v| v.as_str().map(String::from).ok_or(format!("bad {k}")))
            .collect()
    };
    Ok(HomeSnapshot {
        fetched_at: parse_iso(data.get("fetched_at").and_then(Value::as_str).ok_or("bad fetched_at")?)
            .map_err(|e| e.to_string())?,
        locks_unlocked: int("locks_unlocked")?,
        locks_unavailable: int("locks_unavailable")?,
        lights_on: int("lights_on")?,
        gates_open: list("gates_open")?,
        gates_unavailable: list("gates_unavailable")?,
        power_tier: data.get("power_tier").and_then(Value::as_str).map(String::from),
        updates_pending: data.get("updates_pending").and_then(Value::as_u64).unwrap_or(0) as u32,
    })
}

/// POST the template and return the parsed JSON it renders.
pub fn render_template(url: &str, token: &str, template: &str, timeout: Duration) -> Result<Value, HaError> {
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let response = agent
        .post(&format!("{url}/api/template"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .set("User-Agent", "matrix-display/0.2")
        .send_string(&json!({ "template": template }).to_string());
    let body = match response {
        Ok(r) => r.into_string().map_err(|_| HaError::Unreachable("Home Assistant unreachable (read)".into()))?,
        Err(ureq::Error::Status(code, _)) if code == 401 || code == 403 => {
            return Err(HaError::Auth(format!("Home Assistant rejected the token (HTTP {code})")))
        }
        Err(ureq::Error::Status(code, _)) => {
            return Err(HaError::BadResponse(format!("Home Assistant returned HTTP {code}")))
        }
        // The transport error text can hold the URL but never the token (it is only a header).
        Err(ureq::Error::Transport(t)) => {
            return Err(HaError::Unreachable(format!("Home Assistant unreachable ({:?})", t.kind())))
        }
    };
    serde_json::from_str(&body).map_err(|_| HaError::BadResponse("Home Assistant returned invalid JSON".into()))
}

type Render = dyn Fn(&str, &str, &str) -> Result<Value, HaError> + Send + Sync;
type Clock = dyn Fn() -> Dt + Send + Sync;

struct Inner {
    config: HomeAssistantConfig,
    url: String,
    template: String,
    cache_path: Option<PathBuf>,
    clock: Box<Clock>,
    render: Box<Render>,
    snapshot: Mutex<Option<HomeSnapshot>>,
    failures: AtomicU32,
    auth_logged: AtomicBool,
    stop: Signal,
}

pub struct HomeAssistantPoller {
    inner: Arc<Inner>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for HomeAssistantPoller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HomeAssistantPoller(url={:?})", self.inner.url)
    }
}

impl HomeAssistantPoller {
    pub fn new(config: HomeAssistantConfig, cache_path: Option<PathBuf>) -> Result<Self, String> {
        let timeout = Duration::from_secs(TIMEOUT_SECONDS);
        Self::with_parts(
            config,
            cache_path,
            Box::new(|| Utc::now().fixed_offset()),
            Box::new(move |u, t, tpl| render_template(u, t, tpl, timeout)),
        )
    }

    pub fn with_parts(config: HomeAssistantConfig, cache_path: Option<PathBuf>, clock: Box<Clock>,
                      render: Box<Render>) -> Result<Self, String> {
        let url = normalize_url(&config.url).map_err(|e| e.to_string())?;
        let template = build_template(&config.gates, &config.power_tier_entity)?;
        let inner = Inner {
            config,
            url,
            template,
            cache_path,
            clock,
            render,
            snapshot: Mutex::new(None),
            failures: AtomicU32::new(0),
            auth_logged: AtomicBool::new(false),
            stop: Signal::new(),
        };
        inner.load_cache();
        Ok(HomeAssistantPoller { inner: Arc::new(inner), thread: Mutex::new(None) })
    }

    pub fn latest(&self) -> Option<HomeSnapshot> {
        self.inner.snapshot.lock().unwrap().clone()
    }

    pub fn age_seconds(&self) -> Option<f64> {
        let snap = self.latest()?;
        Some((self.inner.clock)().signed_duration_since(snap.fetched_at).num_milliseconds() as f64 / 1000.0)
    }

    pub fn failures(&self) -> u32 {
        self.inner.failures.load(Ordering::Relaxed)
    }

    /// One poll. Returns the new snapshot, or `None` on failure (the last good one is kept).
    pub fn poll_once(&self) -> Option<HomeSnapshot> {
        self.inner.poll_once()
    }

    pub fn next_delay(&self, auth_failed: bool) -> f64 {
        self.inner.next_delay(auth_failed)
    }

    pub fn start(&self) {
        let mut slot = self.thread.lock().unwrap();
        if slot.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        self.inner.stop.clear();
        let inner = Arc::clone(&self.inner);
        *slot = Some(
            std::thread::Builder::new()
                .name("ha-poller".into())
                .spawn(move || {
                    while !inner.stop.is_set() {
                        inner.poll_once();
                        let auth = inner.auth_logged.load(Ordering::Relaxed);
                        inner.stop.wait(Duration::from_secs_f64(inner.next_delay(auth)));
                    }
                })
                .expect("spawn poller thread"),
        );
    }

    pub fn stop(&self) {
        self.inner.stop.set();
        if let Some(t) = self.thread.lock().unwrap().take() {
            join_within(t, Duration::from_secs(TIMEOUT_SECONDS + 2));
        }
    }

    pub fn is_running(&self) -> bool {
        self.thread.lock().unwrap().is_some()
    }
}

impl Inner {
    fn poll_once(&self) -> Option<HomeSnapshot> {
        let result = (self.render)(&self.url, &self.config.token, &self.template)
            .and_then(|data| parse_template_result(&data, &self.config, Some((self.clock)())));
        let snap = match result {
            Ok(s) => s,
            Err(HaError::Auth(msg)) => {
                self.failures.fetch_add(1, Ordering::Relaxed);
                if !self.auth_logged.swap(true, Ordering::Relaxed) {
                    log::error!("{msg}; check MATRIX_HA_TOKEN. Retrying every {}s.", AUTH_RETRY_SECONDS as u32);
                }
                return None;
            }
            Err(e) => {
                let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
                // First failure and then every 10th, so a long outage does not spam the log.
                if failures == 1 || failures % 10 == 0 {
                    log::warn!("{e} (failure {failures}); keeping last data");
                }
                return None;
            }
        };
        let before = self.failures.swap(0, Ordering::Relaxed);
        if before > 0 {
            log::info!("Home Assistant poll recovered after {before} failure(s)");
        }
        self.auth_logged.store(false, Ordering::Relaxed);
        *self.snapshot.lock().unwrap() = Some(snap.clone());
        self.save_cache(&snap);
        Some(snap)
    }

    fn next_delay(&self, auth_failed: bool) -> f64 {
        let failures = self.failures.load(Ordering::Relaxed);
        if auth_failed {
            AUTH_RETRY_SECONDS
        } else if failures == 0 {
            self.config.poll_seconds as f64
        } else {
            (self.config.poll_seconds as f64 * 2f64.powi(failures as i32 - 1)).min(MAX_BACKOFF_SECONDS)
        }
    }

    fn load_cache(&self) {
        let Some(path) = self.cache_path.as_ref().filter(|p| p.is_file()) else {
            return;
        };
        let loaded = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
            .and_then(|v| snapshot_from_json(&v));
        match loaded {
            Ok(s) => *self.snapshot.lock().unwrap() = Some(s),
            Err(_) => log::warn!("ignoring unreadable Home Assistant cache {}", path.display()),
        }
    }

    fn save_cache(&self, snap: &HomeSnapshot) {
        let Some(path) = &self.cache_path else {
            return;
        };
        if let Err(e) = write_atomic(path, &snapshot_to_json(snap).to_string(), ".tmp") {
            log::warn!("could not write Home Assistant cache: {:?}", e.kind());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples::at;
    use chrono::Duration as Dur;
    use std::collections::VecDeque;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    const TOKEN: &str = "eyJfake.token.value-for-tests-only";
    const MIXED: &str = include_str!("../../tests/fixtures/ha_template_mixed.json");
    const QUIET: &str = include_str!("../../tests/fixtures/ha_template_quiet.json");

    fn gates() -> Vec<String> {
        ["binary_sensor.back_gate_door", "cover.side_gate", "binary_sensor.side_gate_door"]
            .iter().map(|s| s.to_string()).collect()
    }

    fn make_config(url: &str) -> HomeAssistantConfig {
        HomeAssistantConfig {
            url: url.into(),
            token: TOKEN.into(),
            gates: gates(),
            exclude_lights: ["light.*_rgb_indicator", "light.*_ap_led", "light.*_led"]
                .iter().map(|s| s.to_string()).collect(),
            ..HomeAssistantConfig::default()
        }
    }

    fn fixture(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn parse(v: &Value, cfg: &HomeAssistantConfig) -> Result<HomeSnapshot, HaError> {
        parse_template_result(v, cfg, Some(at(2026, 1, 1, 0, 0)))
    }

    #[test]
    fn mixed() {
        let snap = parse(&fixture(MIXED), &make_config("http://x")).unwrap();
        assert_eq!(snap.locks_unlocked, 4); // unlocked, jammed, unlocking, unlocked
        assert_eq!(snap.locks_unavailable, 2); // unavailable, unknown
        assert_eq!(snap.lights_on, 3); // kitchen, hall, den; LEDs excluded
        assert_eq!(snap.gates_open, ["binary_sensor.back_gate_door"]);
        assert_eq!(snap.gates_unavailable, ["binary_sensor.side_gate_door"]);
    }

    #[test]
    fn lock_exclusion_glob() {
        let mut cfg = make_config("http://x");
        cfg.exclude_locks = vec!["lock.guest_*".into(), "lock.old_door".into()];
        let snap = parse(&fixture(MIXED), &cfg).unwrap();
        assert_eq!((snap.locks_unlocked, snap.locks_unavailable), (3, 1));
    }

    #[test]
    fn light_glob_is_anchored_and_case_sensitive() {
        let data = json!({"locks": [], "gates": ["off", "off", "off"],
            "lights": ["light.led_strip", "light.desk_led", "light.desk_led_2", "light.x_ap_led"]});
        assert_eq!(parse(&data, &make_config("http://x")).unwrap().lights_on, 2);
    }

    #[test]
    fn cover_states() {
        let data = json!({"locks": [], "lights": [], "gates": ["open", "opening", "closed"]});
        let snap = parse(&data, &make_config("http://x")).unwrap();
        assert_eq!(snap.gates_open, gates()[..2]);
    }

    #[test]
    fn quiet() {
        let snap = parse(&fixture(QUIET), &make_config("http://x")).unwrap();
        assert_eq!((snap.locks_unlocked, snap.lights_on, snap.gates_open.len()), (0, 0, 0));
    }

    #[test]
    fn malformed_shapes() {
        let cases = [
            json!([]), json!({"locks": []}), json!({"locks": 1, "lights": [], "gates": []}),
            json!({"locks": [], "lights": [], "gates": ["on"]}), // wrong gate count
            json!({"locks": [["x"]], "lights": [], "gates": ["on", "on", "on"]}),
        ];
        for bad in cases {
            assert!(matches!(parse(&bad, &make_config("http://x")), Err(HaError::BadResponse(_))), "{bad}");
        }
    }

    #[test]
    fn no_gates_configured() {
        let mut cfg = make_config("http://x");
        cfg.gates.clear();
        let data = json!({"locks": [], "lights": ["light.a"], "gates": []});
        assert_eq!(parse(&data, &cfg).unwrap().lights_on, 1);
    }

    #[test]
    fn snapshot_json_roundtrip() {
        let snap = parse(&fixture(MIXED), &make_config("http://x")).unwrap();
        let text = snapshot_to_json(&snap).to_string();
        assert_eq!(snapshot_from_json(&serde_json::from_str(&text).unwrap()).unwrap(), snap);
    }

    #[test]
    fn power_tier_and_updates() {
        let mut cfg = make_config("http://x");
        let data = json!({"locks": [], "lights": [], "gates": ["off", "off", "off"],
                          "updates": ["update.core", "update.skip_me"], "tier": "on_peak"});
        let snap = parse(&data, &cfg).unwrap();
        assert_eq!((snap.power_tier.clone(), snap.updates_pending), (None, 2));
        cfg.power_tier_entity = "sensor.tier".into();
        cfg.exclude_updates = vec!["update.skip_*".into()];
        let snap = parse(&data, &cfg).unwrap();
        assert_eq!((snap.power_tier.as_deref(), snap.updates_pending), (Some("on_peak"), 1));
        let none = json!({"locks": [], "lights": [], "gates": ["off", "off", "off"], "tier": null});
        assert_eq!(parse(&none, &cfg).unwrap().power_tier.as_deref(), Some("unknown"));
    }

    #[test]
    fn gates_embedded_and_validated() {
        let tpl = build_template(&gates(), "").unwrap();
        for gate in gates() {
            assert!(tpl.contains(&format!("\"{gate}\"")));
        }
        for bad in ["binary_sensor.x\"] }}{{ 1", "binary_sensor.x'", "nodot", "Light.Upper"] {
            assert!(build_template(&[bad.to_string()], "").is_err(), "{bad}");
        }
        assert!(build_template(&[], "sensor.bad tier").is_err());
        assert!(build_template(&[], "sensor.tier").unwrap().contains("\"sensor.tier\""));
    }

    // -- a tiny fake Home Assistant -------------------------------------------------

    type Request = (String, Option<String>, Value);

    struct Server {
        url: String,
        requests: Arc<Mutex<Vec<Request>>>,
        stop: Arc<AtomicBool>,
    }

    impl Server {
        fn start(body: &str, status: u16) -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (reqs, stop2, body) = (Arc::clone(&requests), Arc::clone(&stop), body.to_string());
            std::thread::spawn(move || {
                while !stop2.load(Ordering::Relaxed) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    stream.set_nonblocking(false).unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    let (mut auth, mut len) = (None, 0usize);
                    loop {
                        line.clear();
                        reader.read_line(&mut line).unwrap();
                        if line.trim().is_empty() {
                            break;
                        }
                        let lower = line.to_lowercase();
                        if let Some(v) = lower.strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap();
                        }
                        if lower.starts_with("authorization:") {
                            auth = Some(line.split_once(':').unwrap().1.trim().to_string());
                        }
                    }
                    let mut payload = vec![0u8; len];
                    reader.read_exact(&mut payload).unwrap();
                    reqs.lock().unwrap().push((path, auth.clone(), serde_json::from_slice(&payload).unwrap()));
                    let (status, body) = if auth.as_deref() != Some(&format!("Bearer {TOKEN}")) {
                        (401, String::new())
                    } else {
                        (status, body.clone())
                    };
                    let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                }
            });
            Server { url, requests, stop }
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn dead_url() -> String {
        let s = Server::start("{}", 200);
        let url = s.url.clone();
        drop(s);
        std::thread::sleep(Duration::from_millis(30));
        url
    }

    #[test]
    fn success_and_request_shape() {
        let srv = Server::start(MIXED, 200);
        let poller = HomeAssistantPoller::new(make_config(&format!("{}/", srv.url)), None).unwrap();
        let snap = poller.poll_once().unwrap();
        assert_eq!(snap.lights_on, 3);
        assert_eq!(poller.latest().as_ref(), Some(&snap));
        let reqs = srv.requests.lock().unwrap();
        let (path, auth, payload) = &reqs[0];
        assert_eq!(path, "/api/template");
        assert_eq!(auth.as_deref(), Some(format!("Bearer {TOKEN}").as_str()));
        assert!(payload["template"].as_str().unwrap().contains("states.lock"));
    }

    #[test]
    fn auth_failure_is_reported_without_the_token() {
        let srv = Server::start("{}", 200);
        let mut cfg = make_config(&srv.url);
        cfg.token = "wrong".into();
        let poller = HomeAssistantPoller::new(cfg, None).unwrap();
        for _ in 0..3 {
            assert!(poller.poll_once().is_none());
        }
        assert_eq!(poller.failures(), 3);
        assert_eq!(poller.next_delay(true), AUTH_RETRY_SECONDS);
        let err = render_template(&srv.url, "wrong", "t", Duration::from_secs(5)).unwrap_err();
        assert!(matches!(&err, HaError::Auth(m) if m.contains("401") && !m.contains("wrong")));
    }

    #[test]
    fn failures_are_classified_and_never_leak_the_token() {
        let servers = [Server::start("garbage", 200), Server::start("{}", 500)];
        let cases = [
            (servers[0].url.clone(), "BadResponse"),
            (servers[1].url.clone(), "BadResponse"),
            (dead_url(), "Unreachable"),
        ];
        for (url, kind) in cases {
            let err = render_template(&url, TOKEN, "t", Duration::from_secs(5)).unwrap_err();
            let shown = format!("{err} {err:?}");
            assert!(!shown.contains(TOKEN), "{shown}");
            assert!(format!("{err:?}").starts_with(kind), "{err:?}");
            let poller = HomeAssistantPoller::new(make_config(&url), None).unwrap();
            assert!(!format!("{poller:?}").contains(TOKEN));
            assert!(!format!("{:?}", make_config(&url)).contains(TOKEN));
            assert!(poller.poll_once().is_none());
            assert_eq!(poller.failures(), 1);
        }
    }

    #[test]
    fn valid_json_in_the_wrong_shape_is_a_failure() {
        let srv = Server::start("\"just a string\"", 200);
        let poller = HomeAssistantPoller::new(make_config(&srv.url), None).unwrap();
        assert!(poller.poll_once().is_none());
    }

    fn scripted(results: Vec<Result<Value, HaError>>, now: Arc<Mutex<Dt>>, cache: Option<PathBuf>)
                -> HomeAssistantPoller {
        let queue = Mutex::new(VecDeque::from(results));
        let mut cfg = make_config("http://127.0.0.1:1");
        cfg.poll_seconds = 15;
        HomeAssistantPoller::with_parts(
            cfg,
            cache,
            Box::new(move || *now.lock().unwrap()),
            Box::new(move |_, _, _| queue.lock().unwrap().pop_front().expect("scripted result")),
        )
        .unwrap()
    }

    fn clock() -> Arc<Mutex<Dt>> {
        Arc::new(Mutex::new(at(2026, 1, 1, 0, 0)))
    }

    fn down() -> Result<Value, HaError> {
        Err(HaError::Unreachable("down".into()))
    }

    #[test]
    fn keeps_last_good_and_age() {
        let now = clock();
        let poller = scripted(vec![Ok(fixture(MIXED)), down()], Arc::clone(&now), None);
        let first = poller.poll_once().unwrap();
        *now.lock().unwrap() += Dur::seconds(40);
        assert!(poller.poll_once().is_none());
        assert_eq!(poller.latest().as_ref(), Some(&first));
        assert_eq!(poller.age_seconds(), Some(40.0));
    }

    #[test]
    fn age_none_before_first_poll() {
        assert_eq!(scripted(vec![], clock(), None).age_seconds(), None);
    }

    #[test]
    fn backoff_caps_and_resets() {
        let mut results: Vec<_> = (0..6).map(|_| down()).collect();
        results.push(Ok(fixture(QUIET)));
        let poller = scripted(results, clock(), None);
        let mut delays = vec![poller.next_delay(false)];
        for _ in 0..6 {
            poller.poll_once();
            delays.push(poller.next_delay(false));
        }
        assert_eq!(delays, [15.0, 15.0, 30.0, 60.0, 60.0, 60.0, 60.0]);
        poller.poll_once();
        assert_eq!(poller.next_delay(false), 15.0);
    }

    #[test]
    fn cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ha-cache.json");
        let poller = scripted(vec![Ok(fixture(MIXED))], clock(), Some(path.clone()));
        let snap = poller.poll_once().unwrap();
        assert!(!dir.path().join("ha-cache.json.tmp").exists());
        assert_eq!(scripted(vec![], clock(), Some(path.clone())).latest().as_ref(), Some(&snap));
        std::fs::write(&path, "corrupt").unwrap();
        assert!(scripted(vec![], clock(), Some(path)).latest().is_none());
    }

    #[test]
    fn thread_start_stop() {
        let srv = Server::start(QUIET, 200);
        let mut cfg = make_config(&srv.url);
        cfg.poll_seconds = 1;
        let poller = HomeAssistantPoller::new(cfg, None).unwrap();
        poller.start();
        for _ in 0..50 {
            if poller.latest().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        poller.stop();
        assert!(poller.latest().is_some());
        assert!(!poller.is_running());
    }
}
