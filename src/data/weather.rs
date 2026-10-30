//! Open-Meteo client and background fetcher (DESIGN.md section 5).
//!
//! `parse_open_meteo` turns the JSON response into a `WeatherSnapshot` with timezone-aware
//! datetimes. `WeatherFetcher` refreshes it on a thread, keeps the last good snapshot (also on
//! disk, so a boot without network still shows something) and backs off exponentially, capped at
//! 5 minutes, after failures.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Value};

use super::{iso, parse_iso, write_atomic};
use crate::config::{LocationConfig, WeatherConfig};
use crate::state::*;
use crate::util::{join_within, lock, run_guarded, Signal};

pub const API_URL: &str = "https://api.open-meteo.com/v1/forecast";
pub const USER_AGENT: &str = "matrix-display/0.2 (home LED matrix; +https://open-meteo.com)";
pub const TIMEOUT_SECONDS: u64 = 20;
pub const MAX_BACKOFF_SECONDS: f64 = 300.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WeatherError(pub String);

impl fmt::Display for WeatherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WeatherError {}

pub fn build_url(location: &LocationConfig) -> String {
    let query = [
        ("latitude", format!("{:.3}", location.latitude)),
        ("longitude", format!("{:.3}", location.longitude)),
        ("current", "temperature_2m,weather_code,cloud_cover,wind_speed_10m,is_day".into()),
        ("hourly", "weather_code,temperature_2m,precipitation_probability".into()),
        ("daily", "temperature_2m_max,temperature_2m_min,sunrise,sunset".into()),
        ("temperature_unit", location.units.clone()),
        ("timezone", "auto".into()),
        // Absolute times: hours on the far side of a DST change are still placed correctly.
        ("timeformat", "unixtime".into()),
        ("forecast_days", "2".into()),
    ];
    let q: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{API_URL}?{}", q.join("&"))
}

pub fn new_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(TIMEOUT_SECONDS)).build()
}

pub fn fetch_json(url: &str) -> Result<Value, WeatherError> {
    fetch_json_with(&new_agent(), url)
}

/// GET `url` with an existing agent (the fetcher keeps one for its lifetime).
pub fn fetch_json_with(agent: &ureq::Agent, url: &str) -> Result<Value, WeatherError> {
    let resp = agent
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| WeatherError(format!("weather request failed: {e}")))?;
    resp.into_json::<Value>().map_err(|e| WeatherError(format!("weather request failed: {e}")))
}

/// Open-Meteo times are local wall-clock strings without an offset.
fn local(text: &str, tz: FixedOffset) -> Result<Dt, WeatherError> {
    let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S"))
        .map_err(|e| WeatherError(format!("bad time {text:?}: {e}")))?;
    tz.from_local_datetime(&naive)
        .single()
        .ok_or_else(|| WeatherError(format!("bad time {text:?}")))
}

/// A time from the response: Unix seconds (`timeformat=unixtime`, what we ask for) or, for older
/// recordings, a local wall-clock string.
fn time_value(v: &Value, tz: FixedOffset, what: &str) -> Result<Dt, WeatherError> {
    match v {
        Value::Number(n) => {
            let secs = n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).ok_or_else(|| bad(what))?;
            DateTime::from_timestamp(secs, 0).map(|t| t.with_timezone(&tz)).ok_or_else(|| bad(what))
        }
        Value::String(text) => local(text, tz),
        _ => Err(bad(what)),
    }
}

/// The local calendar date of a daily entry's `time` (`"2026-10-24"`, or local midnight in Unix seconds).
fn day_of(v: &Value, tz: FixedOffset) -> Option<chrono::NaiveDate> {
    match v {
        Value::String(text) => chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok(),
        other => time_value(other, tz, "daily time").ok().map(|t| t.date_naive()),
    }
}

fn bad(what: &str) -> WeatherError {
    WeatherError(format!("unexpected Open-Meteo response: {what}"))
}

fn num(v: &Value, what: &str) -> Result<f64, WeatherError> {
    v.as_f64().ok_or_else(|| bad(what))
}

fn int(v: &Value, what: &str) -> Result<i64, WeatherError> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).ok_or_else(|| bad(what)),
        Value::Bool(b) => Ok(*b as i64),
        _ => Err(bad(what)),
    }
}

fn truthy(v: &Value, what: &str) -> Result<bool, WeatherError> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => Ok(n.as_f64().is_some_and(|f| f != 0.0)),
        _ => Err(bad(what)),
    }
}

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a Value, WeatherError> {
    v.get(key).ok_or_else(|| bad(&format!("missing {key:?}")))
}

fn list<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, WeatherError> {
    field(v, key)?.as_array().ok_or_else(|| bad(&format!("{key:?} is not a list")))
}

/// Convert an Open-Meteo response to a snapshot. Errors on a bad payload.
pub fn parse_open_meteo(data: &Value, fetched_at: Option<Dt>) -> Result<WeatherSnapshot, WeatherError> {
    let offset = int(field(data, "utc_offset_seconds")?, "utc_offset_seconds")?;
    let tz = FixedOffset::east_opt(offset as i32).ok_or_else(|| bad("utc_offset_seconds"))?;
    let cur = field(data, "current")?;
    let current = CurrentWeather {
        temperature: num(field(cur, "temperature_2m")?, "temperature_2m")?,
        code: int(field(cur, "weather_code")?, "weather_code")?,
        cloud_cover: int(field(cur, "cloud_cover")?, "cloud_cover")?,
        wind_speed: num(field(cur, "wind_speed_10m")?, "wind_speed_10m")?,
        is_day: truthy(field(cur, "is_day")?, "is_day")?,
    };
    let hourly = field(data, "hourly")?;
    let (times, codes, temps, probs) = (
        list(hourly, "time")?,
        list(hourly, "weather_code")?,
        list(hourly, "temperature_2m")?,
        list(hourly, "precipitation_probability")?,
    );
    let mut points = Vec::new();
    for (((t, code), temp), prob) in times.iter().zip(codes).zip(temps).zip(probs) {
        if code.is_null() || temp.is_null() {
            continue;
        }
        points.push(HourlyPoint {
            time: time_value(t, tz, "hourly time")?,
            code: int(code, "hourly weather_code")?,
            temperature: num(temp, "hourly temperature_2m")?,
            precipitation_probability: if prob.is_null() { 0 } else { int(prob, "precipitation_probability")? },
        });
    }
    let fetched_at = fetched_at.unwrap_or_else(|| Utc::now().with_timezone(&tz));
    let daily = field(data, "daily")?;
    // Today's entry, by the location's local date (the response's own "current" time, else the
    // fetch time). Open-Meteo starts with today, so this is normally entry 0.
    let today_date = match cur.get("time") {
        Some(t) => time_value(t, tz, "current time")?,
        None => fetched_at.with_timezone(&tz),
    }
    .date_naive();
    let index = daily
        .get("time")
        .and_then(Value::as_array)
        .and_then(|days| days.iter().position(|d| day_of(d, tz) == Some(today_date)))
        .unwrap_or(0);
    let pick = |key: &str| -> Result<&Value, WeatherError> {
        let values = list(daily, key)?;
        values.get(index).or(values.first()).ok_or_else(|| bad(&format!("{key:?} is empty")))
    };
    let today = DailyWeather {
        low: num(pick("temperature_2m_min")?, "temperature_2m_min")?,
        high: num(pick("temperature_2m_max")?, "temperature_2m_max")?,
        sunrise: time_value(pick("sunrise")?, tz, "sunrise")?,
        sunset: time_value(pick("sunset")?, tz, "sunset")?,
    };
    Ok(WeatherSnapshot {
        fetched_at,
        current,
        hourly: points,
        daily: today,
    })
}

pub fn snapshot_to_json(s: &WeatherSnapshot) -> Value {
    json!({
        "fetched_at": iso(&s.fetched_at),
        "current": {
            "temperature": s.current.temperature, "code": s.current.code,
            "cloud_cover": s.current.cloud_cover,
            "wind_speed": s.current.wind_speed, "is_day": s.current.is_day,
        },
        "hourly": s.hourly.iter().map(|p| json!({
            "time": iso(&p.time), "code": p.code, "temperature": p.temperature,
            "precipitation_probability": p.precipitation_probability,
        })).collect::<Vec<_>>(),
        "daily": {
            "low": s.daily.low, "high": s.daily.high,
            "sunrise": iso(&s.daily.sunrise), "sunset": iso(&s.daily.sunset),
        },
    })
}

pub fn snapshot_from_json(data: &Value) -> Result<WeatherSnapshot, WeatherError> {
    let err = |e: &dyn fmt::Debug| WeatherError(format!("bad weather cache: {e:?}"));
    let time = |v: &Value| -> Result<DateTime<FixedOffset>, WeatherError> {
        parse_iso(v.as_str().ok_or_else(|| err(&"time"))?).map_err(|e| err(&e))
    };
    let get = |v: &Value, k: &str| -> Result<Value, WeatherError> {
        v.get(k).cloned().ok_or_else(|| err(&format!("missing {k}")))
    };
    let cur = get(data, "current")?;
    let daily = get(data, "daily")?;
    let hourly = get(data, "hourly")?;
    let hourly = hourly.as_array().ok_or_else(|| err(&"hourly"))?;
    Ok(WeatherSnapshot {
        fetched_at: time(&get(data, "fetched_at")?)?,
        current: CurrentWeather {
            temperature: num(&get(&cur, "temperature")?, "temperature").map_err(|e| err(&e))?,
            code: int(&get(&cur, "code")?, "code").map_err(|e| err(&e))?,
            cloud_cover: int(&get(&cur, "cloud_cover")?, "cloud_cover").map_err(|e| err(&e))?,
            wind_speed: num(&get(&cur, "wind_speed")?, "wind_speed").map_err(|e| err(&e))?,
            is_day: truthy(&get(&cur, "is_day")?, "is_day").map_err(|e| err(&e))?,
        },
        hourly: hourly
            .iter()
            .map(|p| {
                Ok(HourlyPoint {
                    time: time(&get(p, "time")?)?,
                    code: int(&get(p, "code")?, "code").map_err(|e| err(&e))?,
                    temperature: num(&get(p, "temperature")?, "temperature").map_err(|e| err(&e))?,
                    precipitation_probability: int(&get(p, "precipitation_probability")?, "prob")
                        .map_err(|e| err(&e))?,
                })
            })
            .collect::<Result<_, WeatherError>>()?,
        daily: DailyWeather {
            low: num(&get(&daily, "low")?, "low").map_err(|e| err(&e))?,
            high: num(&get(&daily, "high")?, "high").map_err(|e| err(&e))?,
            sunrise: time(&get(&daily, "sunrise")?)?,
            sunset: time(&get(&daily, "sunset")?)?,
        },
    })
}

pub type FetchJson = dyn Fn(&str) -> Result<Value, WeatherError> + Send + Sync;

pub fn fetch_weather(location: &LocationConfig, fetcher: &dyn Fn(&str) -> Result<Value, WeatherError>) -> Result<WeatherSnapshot, WeatherError> {
    let data = fetcher(&build_url(location))?; // network, HTTP, JSON: all mean "keep the old data"
    parse_open_meteo(&data, None)
}

/// Delay before retry number `failures` (1-based): 15, 30, 60, 120, 240, 300, 300...
pub fn backoff_seconds(failures: u32) -> f64 {
    MAX_BACKOFF_SECONDS.min(15.0 * 2f64.powi(failures.saturating_sub(1) as i32))
}

type Fetch = dyn Fn(&LocationConfig) -> Result<WeatherSnapshot, WeatherError> + Send + Sync;

struct Shared {
    latest: Option<Arc<WeatherSnapshot>>,
    failures: u32,
}

struct Inner {
    location: LocationConfig,
    refresh: u64,
    cache_path: Option<PathBuf>,
    fetch: Box<Fetch>,
    shared: Mutex<Shared>,
    stop: Signal,
}

/// Refreshes weather on a background thread; `latest()` is safe to call from any thread.
pub struct WeatherFetcher {
    inner: Arc<Inner>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl WeatherFetcher {
    pub fn new(location: LocationConfig, config: WeatherConfig, cache_path: Option<PathBuf>) -> Self {
        // One agent for the fetcher's lifetime: it keeps the connection pool and TLS setup.
        let agent = new_agent();
        let fetch: Box<Fetch> = Box::new(move |loc| fetch_weather(loc, &|url| fetch_json_with(&agent, url)));
        Self::with_fetch(location, config, cache_path, fetch)
    }

    pub fn with_fetch(location: LocationConfig, config: WeatherConfig, cache_path: Option<PathBuf>,
                      fetch: Box<Fetch>) -> Self {
        let inner = Inner {
            location,
            refresh: config.refresh_seconds,
            cache_path,
            fetch,
            shared: Mutex::new(Shared { latest: None, failures: 0 }),
            stop: Signal::new(),
        };
        inner.load_cache();
        WeatherFetcher { inner: Arc::new(inner), thread: Mutex::new(None) }
    }

    pub fn latest(&self) -> Option<Arc<WeatherSnapshot>> {
        lock(&self.inner.shared).latest.clone()
    }

    pub fn failures(&self) -> u32 {
        lock(&self.inner.shared).failures
    }

    /// Fetch now. True on success; on failure the last good snapshot is kept.
    pub fn refresh_once(&self) -> bool {
        self.inner.refresh_once()
    }

    pub fn next_delay(&self) -> f64 {
        self.inner.next_delay()
    }

    pub fn start(&self) {
        let mut slot = lock(&self.thread);
        if slot.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        self.inner.stop.clear();
        let inner = Arc::clone(&self.inner);
        *slot = Some(
            std::thread::Builder::new()
                .name("weather".into())
                .spawn(move || {
                    while !inner.stop.is_set() {
                        // A bug in parsing must not end the feed for good: count it as a failure.
                        if run_guarded("weather refresh", || inner.refresh_once()).is_none() {
                            inner.note_failure();
                        }
                        inner.stop.wait(Duration::from_secs_f64(inner.next_delay()));
                    }
                })
                .expect("spawn weather thread"),
        );
    }

    /// Ask the thread to end without waiting for it (it finishes its current request first).
    pub fn request_stop(&self) {
        self.inner.stop.set();
    }

    pub fn stop(&self) {
        self.request_stop();
        if let Some(t) = lock(&self.thread).take() {
            join_within(t, Duration::from_secs(5));
        }
    }
}

impl Inner {
    fn note_failure(&self) -> u32 {
        let mut s = lock(&self.shared);
        s.failures += 1;
        s.failures
    }

    fn refresh_once(&self) -> bool {
        match (self.fetch)(&self.location) {
            Err(e) => {
                let failures = self.note_failure();
                log::warn!("weather refresh failed ({failures} in a row): {e}");
                false
            }
            Ok(snapshot) => {
                let snapshot = Arc::new(snapshot);
                {
                    let mut s = lock(&self.shared);
                    s.latest = Some(Arc::clone(&snapshot));
                    s.failures = 0;
                }
                // A fetch finishing after a reload asked it to stop must not overwrite the new feed's cache.
                if !self.stop.is_set() {
                    self.save_cache(&snapshot);
                }
                true
            }
        }
    }

    fn next_delay(&self) -> f64 {
        let failures = lock(&self.shared).failures;
        if failures == 0 {
            self.refresh as f64
        } else {
            backoff_seconds(failures)
        }
    }

    fn load_cache(&self) {
        let Some(path) = self.cache_path.as_ref().filter(|p| p.is_file()) else {
            return;
        };
        let loaded = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
            .and_then(|v| snapshot_from_json(&v).map_err(|e| e.to_string()));
        match loaded {
            Ok(s) => lock(&self.shared).latest = Some(Arc::new(s)),
            Err(e) => log::warn!("ignoring unreadable weather cache {}: {e}", path.display()),
        }
    }

    fn save_cache(&self, snapshot: &WeatherSnapshot) {
        let Some(path) = &self.cache_path else {
            return;
        };
        let text = snapshot_to_json(snapshot).to_string();
        if let Err(e) = write_atomic(path, &text) {
            log::warn!("could not write weather cache: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forecast::{daily_low_high, next_change};
    use crate::gfx::pyround;
    use std::collections::VecDeque;
    use std::time::Instant;

    const FIXTURE: &str = include_str!("../../tests/fixtures/weather_open_meteo_recorded.json");

    fn data() -> Value {
        serde_json::from_str(FIXTURE).unwrap()
    }

    fn loc() -> LocationConfig {
        let mut l = LocationConfig::new(40.748, -73.994);
        l.postal_code = "10001".into();
        l
    }

    #[test]
    fn build_url_uses_location_and_units() {
        let url = build_url(&loc());
        assert!(url.contains("latitude=40.748") && url.contains("longitude=-73.994"));
        assert!(url.contains("temperature_unit=fahrenheit") && url.contains("timezone=auto"));
        assert!(url.contains("forecast_days=2") && url.contains("is_day"));
        assert!(url.contains("timeformat=unixtime"));
        let mut c = LocationConfig::new(1.0, 2.0);
        c.units = "celsius".into();
        assert!(build_url(&c).contains("celsius"));
    }

    #[test]
    fn parse_recorded_response() {
        let d = data();
        let snap = parse_open_meteo(&d, None).unwrap();
        assert_eq!(snap.current.temperature, d["current"]["temperature_2m"].as_f64().unwrap());
        assert_eq!(snap.current.is_day, d["current"]["is_day"].as_i64().unwrap() != 0);
        assert_eq!(snap.hourly.len(), 48);
        assert_eq!(snap.hourly[0].time.offset().local_minus_utc() as i64,
                   d["utc_offset_seconds"].as_i64().unwrap());
        assert!(snap.daily.sunrise < snap.daily.sunset);
        let (low, high) = daily_low_high(&snap);
        assert_eq!(low, pyround(d["daily"]["temperature_2m_min"][0].as_f64().unwrap()));
        assert_eq!(high, pyround(d["daily"]["temperature_2m_max"][0].as_f64().unwrap()));
    }

    #[test]
    fn next_change_runs_on_parsed_snapshot() {
        let snap = parse_open_meteo(&data(), None).unwrap();
        let first = snap.hourly[0].time;
        if let Some(r) = next_change(&snap, first) {
            assert!(r.time > first);
        }
    }

    #[test]
    fn parse_rejects_bad_payload() {
        let mut d = data();
        d.as_object_mut().unwrap().remove("current");
        assert!(parse_open_meteo(&d, None).is_err());
        assert!(parse_open_meteo(&json!({}), None).is_err());
    }

    #[test]
    fn parse_skips_null_hours() {
        let mut d = data();
        d["hourly"]["weather_code"][3] = Value::Null;
        assert_eq!(parse_open_meteo(&d, None).unwrap().hourly.len(), 47);
    }

    /// The recorded response as `timeformat=unixtime` would have returned it.
    fn unixtime(d: &Value) -> Value {
        let mut d = d.clone();
        let tz = FixedOffset::east_opt(d["utc_offset_seconds"].as_i64().unwrap() as i32).unwrap();
        let unix = |v: &Value| json!(local(v.as_str().unwrap(), tz).unwrap().timestamp());
        let day = |v: &Value| {
            let date = chrono::NaiveDate::parse_from_str(v.as_str().unwrap(), "%Y-%m-%d").unwrap();
            json!(tz.from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap()).unwrap().timestamp())
        };
        d["current"]["time"] = unix(&d["current"]["time"]);
        for (section, key, f) in [("hourly", "time", &unix as &dyn Fn(&Value) -> Value), ("daily", "sunrise", &unix),
                                  ("daily", "sunset", &unix), ("daily", "time", &day)] {
            let converted: Vec<Value> = d[section][key].as_array().unwrap().iter().map(f).collect();
            d[section][key] = json!(converted);
        }
        d
    }

    #[test]
    fn unixtime_response_gives_the_same_snapshot() {
        let at = Some(Utc::now().fixed_offset());
        let iso = parse_open_meteo(&data(), at).unwrap();
        let unix = parse_open_meteo(&unixtime(&data()), at).unwrap();
        assert_eq!(unix, iso);
        assert_eq!(unix.hourly[0].time.offset().local_minus_utc(), -4 * 3600);
    }

    #[test]
    fn daily_entry_follows_the_local_date() {
        for d in [data(), unixtime(&data())] {
            let today = parse_open_meteo(&d, None).unwrap();
            assert_eq!(today.daily.high, 61.2); // the recording's first day
            let mut next = d.clone();
            // the same response read just after midnight: the second day applies
            next["current"]["time"] = if next["current"]["time"].is_string() {
                json!("2026-10-25T00:15")
            } else {
                json!(next["current"]["time"].as_i64().unwrap() + 21 * 3600)
            };
            let tomorrow = parse_open_meteo(&next, None).unwrap();
            assert_eq!((tomorrow.daily.low, tomorrow.daily.high), (38.1, 64.0));
            assert_eq!(tomorrow.daily.sunrise.date_naive().to_string(), "2026-10-25");
            // a date that is not in the response falls back to the first day
            let mut odd = d.clone();
            odd["daily"]["time"] = json!(["2020-01-01", "2020-01-02"]);
            assert_eq!(parse_open_meteo(&odd, None).unwrap().daily.high, 61.2);
        }
    }

    #[test]
    fn a_panicking_fetch_does_not_end_the_thread() {
        let good = parse_open_meteo(&data(), None).unwrap();
        let g = good.clone();
        let calls = std::sync::atomic::AtomicU32::new(0);
        let f = WeatherFetcher::with_fetch(
            loc(), WeatherConfig { refresh_seconds: 3600 }, None,
            Box::new(move |_| {
                if calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                    panic!("bug in a fetch");
                }
                Ok(g.clone())
            }),
        );
        assert_eq!(f.next_delay(), 3600.0);
        f.start();
        // the panic counts as a failure: the retry comes after the first back-off step (15 s),
        // so check the state rather than wait for it
        let deadline = Instant::now() + Duration::from_secs(5);
        while f.failures() == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!((f.failures(), f.next_delay()), (1, 15.0));
        assert!(f.refresh_once()); // the fetcher still works
        assert_eq!(f.latest().as_deref(), Some(&good));
        f.stop();
    }

    #[test]
    fn json_roundtrip() {
        let snap = parse_open_meteo(&data(), None).unwrap();
        let text = snapshot_to_json(&snap).to_string();
        assert_eq!(snapshot_from_json(&serde_json::from_str(&text).unwrap()).unwrap(), snap);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let got: Vec<f64> = [1, 2, 3, 4, 5, 6, 9].iter().map(|n| backoff_seconds(*n)).collect();
        assert_eq!(got, [15.0, 30.0, 60.0, 120.0, 240.0, 300.0, 300.0]);
    }

    #[test]
    fn fetch_weather_wraps_network_errors() {
        let boom: &FetchJson = &|_| Err(WeatherError("offline".into()));
        assert!(fetch_weather(&loc(), boom).is_err());
    }

    #[test]
    fn fetch_weather_with_fake_transport() {
        let d = data();
        let fake: &FetchJson = &|_| Ok(data());
        let snap = fetch_weather(&loc(), fake).unwrap();
        assert_eq!(snap.current.code, d["current"]["weather_code"].as_i64().unwrap());
    }

    fn scripted(results: Vec<Result<WeatherSnapshot, WeatherError>>) -> Box<Fetch> {
        let queue = Mutex::new(VecDeque::from(results));
        Box::new(move |_| queue.lock().unwrap().pop_front().expect("scripted result"))
    }

    #[test]
    fn fetcher_keeps_last_good_and_backs_off() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache.json");
        let good = parse_open_meteo(&data(), None).unwrap();
        let offline = || Err(WeatherError("offline".into()));
        let f = WeatherFetcher::with_fetch(
            loc(), WeatherConfig { refresh_seconds: 900 }, Some(cache.clone()),
            scripted(vec![Ok(good.clone()), offline(), offline()]),
        );
        assert!(f.latest().is_none());
        assert!(f.refresh_once());
        assert_eq!(f.latest().as_deref(), Some(&good));
        assert_eq!(f.next_delay(), 900.0);
        assert!(!f.refresh_once());
        assert_eq!(f.latest().as_deref(), Some(&good));
        assert_eq!((f.failures(), f.next_delay()), (1, 15.0));
        assert!(!f.refresh_once());
        assert_eq!(f.next_delay(), 30.0);

        // a restart while offline loads the cached snapshot
        let restarted = WeatherFetcher::with_fetch(
            loc(), WeatherConfig::default(), Some(cache), scripted(vec![offline()]));
        assert_eq!(restarted.latest().as_deref(), Some(&good));
    }

    #[test]
    fn corrupt_cache_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache.json");
        std::fs::write(&cache, "{not json").unwrap();
        let f = WeatherFetcher::with_fetch(loc(), WeatherConfig::default(), Some(cache), scripted(vec![]));
        assert!(f.latest().is_none());
    }

    #[test]
    fn background_thread_publishes() {
        let dir = tempfile::tempdir().unwrap();
        let good = parse_open_meteo(&data(), None).unwrap();
        let g = good.clone();
        let f = WeatherFetcher::with_fetch(
            loc(), WeatherConfig { refresh_seconds: 3600 }, Some(dir.path().join("c.json")),
            Box::new(move |_| Ok(g.clone())),
        );
        f.start();
        let deadline = Instant::now() + Duration::from_secs(5);
        while f.latest().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(f.latest().as_deref(), Some(&good));
        f.stop();
    }
}
