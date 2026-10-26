//! The live loop: data threads feed a state snapshot, the compositor draws it, the display shows it.
//!
//! Design points (DESIGN.md section 6): the render loop never waits on the network; a bug in a
//! widget must not blank the panel; systemd's watchdog restarts the process if the loop itself hangs.

use std::fmt::Write as _;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use chrono::Local;

use crate::compositor::{brightness_for, Compositor};
use crate::config::Config;
use crate::data::homeassistant::HomeAssistantPoller;
use crate::data::weather::WeatherFetcher;
use crate::display::Display;
use crate::gfx::Img;
use crate::state::{AppState, Dt, HomeSnapshot, WeatherSnapshot};
use crate::util::{flag_is_set, Flag};

/// Seconds between brightness recalculations.
pub const BRIGHTNESS_EVERY: f64 = 30.0;
/// Seconds between performance log lines.
pub const STATS_EVERY: f64 = 60.0;
/// About 10 s at 15 fps, then let systemd restart us.
pub const MAX_CONSECUTIVE_RENDER_FAILURES: u32 = 150;

/// Where caches live: $MATRIX_STATE_DIR, else /var/lib/matrix-display when writable, else ~/.cache.
pub fn state_dir() -> PathBuf {
    if let Ok(env) = std::env::var("MATRIX_STATE_DIR") {
        if !env.is_empty() {
            return PathBuf::from(env);
        }
    }
    let system = Path::new("/var/lib/matrix-display");
    if system.is_dir() && is_writable(system) {
        return system.to_path_buf();
    }
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    home.join(".cache").join("matrix-display")
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".write-probe-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Tell systemd about readiness / liveness (no-op when not run by systemd).
pub fn sd_notify(message: &str) {
    if let Ok(address) = std::env::var("NOTIFY_SOCKET") {
        if !address.is_empty() {
            sd_notify_to(&address, message);
        }
    }
}

#[cfg(unix)]
pub fn sd_notify_to(address: &str, message: &str) {
    use std::os::unix::net::UnixDatagram;
    let Ok(sock) = UnixDatagram::unbound() else { return };
    let sent = if let Some(name) = address.strip_prefix('@') {
        #[cfg(target_os = "linux")]
        {
            use std::os::linux::net::SocketAddrExt;
            std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())
                .and_then(|a| sock.connect_addr(&a))
                .and_then(|_| sock.send(message.as_bytes()))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            return;
        }
    } else {
        sock.connect(address).and_then(|_| sock.send(message.as_bytes()))
    };
    let _ = sent; // best effort
}

#[cfg(not(unix))]
pub fn sd_notify_to(_address: &str, _message: &str) {}

/// The background data feeds, however they are provided.
pub trait DataSources {
    fn start(&mut self);
    fn stop(&mut self);
    fn latest(&self) -> (Option<WeatherSnapshot>, Option<HomeSnapshot>);
}

/// The two live feeds. Either may be absent (no location / no HA token).
pub struct Sources {
    weather: Option<WeatherFetcher>,
    home: Option<HomeAssistantPoller>,
}

impl Sources {
    pub fn new(cfg: &Config, cache_dir: &Path) -> Result<Sources> {
        std::fs::create_dir_all(cache_dir)?;
        let weather = match &cfg.location {
            Some(loc) => Some(WeatherFetcher::new(loc.clone(), cfg.weather.clone(), Some(cache_dir.join("weather.json")))),
            None => {
                log::warn!("no [location] in config: weather disabled (run matrix-geocode <ZIP>)");
                None
            }
        };
        let home = if cfg.homeassistant.token.is_empty() {
            log::warn!("no MATRIX_HA_TOKEN: Home Assistant row disabled");
            None
        } else {
            match HomeAssistantPoller::new(cfg.homeassistant.clone(), Some(cache_dir.join("home.json"))) {
                Ok(p) => Some(p),
                Err(e) => bail!("[homeassistant]: {e}"),
            }
        };
        Ok(Sources { weather, home })
    }
}

impl DataSources for Sources {
    fn start(&mut self) {
        if let Some(w) = &self.weather {
            w.start();
        }
        if let Some(h) = &self.home {
            h.start();
        }
    }

    fn stop(&mut self) {
        if let Some(w) = &self.weather {
            w.stop();
        }
        if let Some(h) = &self.home {
            h.stop();
        }
    }

    fn latest(&self) -> (Option<WeatherSnapshot>, Option<HomeSnapshot>) {
        (self.weather.as_ref().and_then(|w| w.latest()), self.home.as_ref().and_then(|h| h.latest()))
    }
}

/// Fixed data for demos and tests (`--sample`, `--offline`).
pub struct StaticSources {
    weather: Option<WeatherSnapshot>,
    home: Option<HomeSnapshot>,
}

impl StaticSources {
    pub fn new(weather: Option<WeatherSnapshot>, home: Option<HomeSnapshot>) -> Self {
        StaticSources { weather, home }
    }
}

impl DataSources for StaticSources {
    fn start(&mut self) {}

    fn stop(&mut self) {}

    fn latest(&self) -> (Option<WeatherSnapshot>, Option<HomeSnapshot>) {
        (self.weather.clone(), self.home.clone())
    }
}

#[derive(Default)]
pub struct FrameStats {
    pub count: u64,
    pub total_ms: f64,
    pub max_ms: f64,
    pub overruns: u64,
}

impl FrameStats {
    pub fn reset(&mut self) {
        *self = FrameStats::default();
    }

    pub fn record(&mut self, ms: f64, overrun: bool) {
        self.count += 1;
        self.total_ms += ms;
        self.max_ms = self.max_ms.max(ms);
        self.overruns += overrun as u64;
    }

    pub fn summary(&self) -> String {
        let avg = if self.count > 0 { self.total_ms / self.count as f64 } else { 0.0 };
        format!("frames={} render avg={avg:.1}ms max={:.1}ms overruns={}", self.count, self.max_ms, self.overruns)
    }
}

/// What asking `vcgencmd get_throttled` gave.
pub enum Vcgencmd {
    Output(String),
    /// A busy moment: try again next time.
    Timeout,
    Failed,
}

fn run_vcgencmd() -> Vcgencmd {
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new("vcgencmd")
        .arg("get_throttled")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vcgencmd::Failed;
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Vcgencmd::Timeout;
            }
            Err(_) => return Vcgencmd::Failed,
        }
    }
    match child.wait_with_output() {
        Ok(out) => Vcgencmd::Output(String::from_utf8_lossy(&out.stdout).trim().to_string()),
        Err(_) => Vcgencmd::Failed,
    }
}

/// Cleared once vcgencmd fails, so it is not re-run every minute.
static THROTTLE_READABLE: AtomicBool = AtomicBool::new(true);

fn kb_field(text: &str, name: &str) -> Option<u64> {
    text.lines().find_map(|l| l.strip_prefix(name)?.split_whitespace().next()?.parse().ok())
}

/// RSS, CPU temperature and throttle flags for the log (all best-effort).
pub fn system_health() -> String {
    system_health_with(&THROTTLE_READABLE, &run_vcgencmd)
}

pub fn system_health_with(readable: &AtomicBool, vcgencmd: &dyn Fn() -> Vcgencmd) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(kb) = kb_field(&status, "VmRSS:") {
            parts.push(format!("rss={}MB", kb / 1024));
        }
    }
    if let Ok(mem) = std::fs::read_to_string("/proc/meminfo") {
        if let Some(kb) = kb_field(&mem, "MemAvailable:") {
            parts.push(format!("mem_avail={}MB", kb / 1024));
        }
    }
    if let Ok(t) = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
        if let Ok(milli) = t.trim().parse::<f64>() {
            parts.push(format!("temp={:.0}C", milli / 1000.0));
        }
    }
    if readable.load(Ordering::Relaxed) {
        match vcgencmd() {
            Vcgencmd::Timeout => {}
            // unreadable (empty) once root privileges are dropped
            Vcgencmd::Output(out) if out.starts_with("throttled=") => parts.push(out),
            Vcgencmd::Output(_) | Vcgencmd::Failed => readable.store(false, Ordering::Relaxed),
        }
    }
    parts.join(" ")
}

type Reload = dyn FnMut() -> Result<(Config, Box<dyn DataSources>), String>;

pub struct Runner<D: Display> {
    pub cfg: Config,
    pub display: D,
    pub sources: Box<dyn DataSources>,
    pub compositor: Compositor,
    clock: Box<dyn Fn() -> Dt>,
    monotonic: Box<dyn FnMut() -> f64>,
    sleep: Box<dyn FnMut(f64)>,
    notify: Box<dyn FnMut(&str)>,
    reload: Option<Box<Reload>>,
    pub stop: Flag,
    pub reload_requested: Flag,
    pub stats: FrameStats,
    failures: u32,
}

impl<D: Display> Runner<D> {
    pub fn new(cfg: Config, display: D, sources: Box<dyn DataSources>, compositor: Compositor) -> Self {
        let start = Instant::now();
        Runner {
            cfg,
            display,
            sources,
            compositor,
            clock: Box::new(|| Local::now().fixed_offset()),
            monotonic: Box::new(move || start.elapsed().as_secs_f64()),
            sleep: Box::new(|s| std::thread::sleep(Duration::from_secs_f64(s))),
            notify: Box::new(sd_notify),
            reload: None,
            stop: Flag::default(),
            reload_requested: Flag::default(),
            stats: FrameStats::default(),
            failures: 0,
        }
    }

    pub fn with_clock(mut self, clock: Box<dyn Fn() -> Dt>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_timing(mut self, monotonic: Box<dyn FnMut() -> f64>, sleep: Box<dyn FnMut(f64)>) -> Self {
        self.monotonic = monotonic;
        self.sleep = sleep;
        self
    }

    pub fn with_notify(mut self, notify: Box<dyn FnMut(&str)>) -> Self {
        self.notify = notify;
        self
    }

    pub fn with_reload(mut self, reload: Box<Reload>) -> Self {
        self.reload = Some(reload);
        self
    }

    // -- state ------------------------------------------------------------------------
    pub fn build_state(&self) -> AppState {
        let (weather, home) = self.sources.latest();
        let mut now = (self.clock)();
        if let Some(w) = &weather {
            // show the date/time of the forecast location
            now = now.with_timezone(&w.daily.sunrise.timezone());
        }
        AppState { now, weather, home }
    }

    // -- loop -------------------------------------------------------------------------
    pub fn run(&mut self, max_frames: Option<u64>) -> Result<()> {
        let fps = self.cfg.display.scene_fps.max(1) as f64;
        let period = 1.0 / fps;
        let start = (self.monotonic)();
        let (mut next_frame, mut last_stats, mut last_ping) = (start, start, start);
        let mut last_brightness = start - BRIGHTNESS_EVERY; // force an update on the first frame
        let mut last_good: Option<Img> = None;
        let mut frames = 0u64;
        (self.notify)("READY=1");
        log::info!("running at {fps} fps, brightness cap {}%", self.cfg.display.max_brightness);
        while !flag_is_set(&self.stop) && max_frames.is_none_or(|m| frames < m) {
            let now_m = (self.monotonic)();
            if flag_is_set(&self.reload_requested) {
                self.do_reload();
            }
            let state = self.build_state();
            if now_m - last_brightness >= BRIGHTNESS_EVERY {
                let daily = state.weather.as_ref().map(|w| &w.daily);
                self.display.set_brightness(brightness_for(state.now, daily, &self.cfg.display));
                last_brightness = now_m;
            }
            let began = (self.monotonic)();
            let compositor = &mut self.compositor;
            let rendered = catch_unwind(AssertUnwindSafe(|| compositor.render(&state, now_m - start)));
            let frame = match rendered {
                Ok(frame) => {
                    last_good = Some(frame.clone());
                    self.failures = 0;
                    Some(frame)
                }
                Err(_) => {
                    self.failures += 1;
                    if self.failures == 1 || self.failures % 100 == 0 {
                        log::error!("render failed ({} in a row); showing the last good frame", self.failures);
                    }
                    if self.failures >= MAX_CONSECUTIVE_RENDER_FAILURES {
                        bail!("render failed {} times in a row", self.failures);
                    }
                    last_good.clone()
                }
            };
            if let Some(frame) = frame {
                self.display.show(&frame)?;
            }
            let ms = ((self.monotonic)() - began) * 1000.0;
            next_frame += period;
            let delay = next_frame - (self.monotonic)();
            self.stats.record(ms, delay < 0.0);
            frames += 1;
            if now_m - last_ping >= 1.0 {
                (self.notify)("WATCHDOG=1");
                last_ping = now_m;
            }
            if now_m - last_stats >= STATS_EVERY {
                let mut line = self.stats.summary();
                let _ = write!(line, " {}", system_health());
                log::info!("{line}");
                self.stats.reset();
                last_stats = now_m;
            }
            if delay > 0.0 {
                (self.sleep)(delay);
            } else {
                next_frame = (self.monotonic)(); // fell behind: drop frames, do not spiral
            }
        }
        (self.notify)("STOPPING=1");
        Ok(())
    }

    fn do_reload(&mut self) {
        self.reload_requested.store(false, Ordering::Relaxed);
        let Some(reload) = self.reload.as_mut() else { return };
        let (cfg, mut sources) = match reload() {
            Ok(v) => v,
            Err(e) => {
                log::error!("reload failed, keeping the current configuration: {e}");
                return;
            }
        };
        self.sources.stop();
        sources.start();
        self.cfg = cfg;
        self.sources = sources;
        log::info!("configuration reloaded");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compositor::build_default_compositor;
    use crate::config::{build_config, Env};
    use crate::samples::{sample_state, NOON};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// Records frames and brightness settings.
    #[derive(Default)]
    struct TestDisplay {
        last_frame: Option<Img>,
        brightness: Rc<RefCell<Vec<u8>>>,
    }

    impl Display for TestDisplay {
        fn width(&self) -> usize {
            64
        }

        fn height(&self) -> usize {
            64
        }

        fn show(&mut self, image: &Img) -> Result<()> {
            self.check(image)?;
            self.last_frame = Some(image.clone());
            Ok(())
        }

        fn set_brightness(&mut self, percent: u8) {
            self.brightness.borrow_mut().push(percent);
        }
    }

    fn cfg(toml: &str) -> Config {
        build_config(&toml.parse().unwrap(), &Env::new(), &Env::new()).unwrap()
    }

    struct Harness {
        runner: Runner<TestDisplay>,
        clock: Rc<Cell<f64>>,
        brightness: Rc<RefCell<Vec<u8>>>,
        notes: Rc<RefCell<Vec<String>>>,
    }

    fn harness(compositor: Option<Compositor>, fps: u32) -> Harness {
        let config = cfg(&format!("[display]\nscene_fps = {fps}\nbrightness = 50\nmax_brightness = 70"));
        let s = sample_state("rain_then_snow");
        let clock = Rc::new(Cell::new(100.0));
        let (c1, c2) = (Rc::clone(&clock), Rc::clone(&clock));
        let notes = Rc::new(RefCell::new(Vec::new()));
        let n = Rc::clone(&notes);
        let display = TestDisplay::default();
        let brightness = Rc::clone(&display.brightness);
        let runner = Runner::new(
            config,
            display,
            Box::new(StaticSources::new(s.weather.clone(), s.home.clone())),
            compositor.unwrap_or_else(|| build_default_compositor(true, false)),
        )
        .with_clock(Box::new(move || s.now))
        .with_timing(Box::new(move || c1.get()), Box::new(move |secs| c2.set(c2.get() + secs)))
        .with_notify(Box::new(move |m| n.borrow_mut().push(m.to_string())));
        Harness { runner, clock, brightness, notes }
    }

    #[test]
    fn runs_requested_frames_at_the_configured_rate() {
        let mut h = harness(None, 15);
        let start = h.clock.get();
        h.runner.run(Some(30)).unwrap();
        let frame = h.runner.display.last_frame.as_ref().unwrap();
        assert_eq!((frame.w, frame.h), (64, 64));
        assert!(!frame.is_blank());
        assert!((h.clock.get() - start - 2.0).abs() < 0.01); // 30 frames at 15 fps
        assert_eq!(h.runner.stats.count, 30);
    }

    #[test]
    fn brightness_set_on_first_frame_and_clamped() {
        let mut h = harness(None, 15);
        h.runner.run(Some(3)).unwrap();
        let b = h.brightness.borrow();
        assert!(!b.is_empty() && (1..=70).contains(&b[0]));
    }

    #[test]
    fn systemd_notifications() {
        let mut h = harness(None, 15);
        h.runner.run(Some(45)).unwrap(); // 3 s => several watchdog pings
        let notes = h.notes.borrow();
        assert_eq!(notes.first().map(String::as_str), Some("READY=1"));
        assert_eq!(notes.last().map(String::as_str), Some("STOPPING=1"));
        assert!(notes.iter().filter(|n| *n == "WATCHDOG=1").count() >= 2);
    }

    #[test]
    fn render_error_keeps_last_good_frame_and_does_not_crash() {
        let mut comp = Compositor::new(false);
        let calls = Rc::new(Cell::new(0));
        comp.register("temps", Box::new(move |frame, rect, _, _| {
            calls.set(calls.get() + 1);
            if calls.get() > 3 {
                panic!("widget bug");
            }
            frame.put(rect.x, rect.y, [255, 0, 0]);
        }))
        .unwrap();
        let mut h = harness(Some(comp), 15);
        h.runner.run(Some(10)).unwrap();
        assert_eq!(h.runner.display.last_frame.as_ref().unwrap().get(0, 35), [255, 0, 0]); // stale but present
    }

    #[test]
    fn persistent_render_failure_errors_so_systemd_can_restart() {
        let mut comp = Compositor::new(false);
        comp.register("temps", Box::new(|_, _, _, _| panic!("always"))).unwrap();
        let mut h = harness(Some(comp), 15);
        assert!(h.runner.run(Some(MAX_CONSECUTIVE_RENDER_FAILURES as u64 + 5)).is_err());
    }

    #[test]
    fn slow_frames_are_dropped_not_queued() {
        let mut comp = Compositor::new(false);
        let clock = Rc::new(Cell::new(100.0));
        let c = Rc::clone(&clock);
        comp.register("temps", Box::new(move |_, _, _, _| c.set(c.get() + 0.5))).unwrap(); // each frame takes 500 ms
        let mut h = harness(Some(comp), 15);
        // the harness clock is separate: share ours with the runner
        let (c1, c2) = (Rc::clone(&clock), Rc::clone(&clock));
        h.runner = h.runner.with_timing(Box::new(move || c1.get()), Box::new(move |s| c2.set(c2.get() + s)));
        h.runner.run(Some(5)).unwrap();
        assert_eq!(h.runner.stats.overruns, 5);
        assert!(clock.get() - 100.0 < 5.0 * 0.5 + 0.1); // never sleeps extra to catch up
    }

    #[test]
    fn stop_flag_ends_loop() {
        let mut h = harness(None, 15);
        h.runner.stop.store(true, Ordering::Relaxed);
        h.runner.run(None).unwrap(); // returns immediately
        assert_eq!(h.runner.stats.count, 0);
    }

    #[test]
    fn reload_swaps_config_and_survives_errors() {
        let new_cfg = cfg("[display]\nscene_fps = 10");
        let s = sample_state("clear_day");
        let seen = Rc::new(Cell::new(0));
        let seen2 = Rc::clone(&seen);
        let mut h = harness(None, 15);
        h.runner = h.runner.with_reload(Box::new(move || {
            seen2.set(seen2.get() + 1);
            if seen2.get() == 1 {
                return Err("disk gone".into());
            }
            Ok((new_cfg.clone(), Box::new(StaticSources::new(s.weather.clone(), s.home.clone())) as Box<dyn DataSources>))
        }));
        h.runner.reload_requested.store(true, Ordering::Relaxed);
        h.runner.run(Some(2)).unwrap();
        assert_eq!(h.runner.cfg.display.scene_fps, 15); // failed reload keeps old config
        h.runner.reload_requested.store(true, Ordering::Relaxed);
        h.runner.run(Some(2)).unwrap();
        assert_eq!(h.runner.cfg.display.scene_fps, 10);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sd_notify_reaches_a_socket() {
        use std::os::unix::net::UnixDatagram;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notify");
        let server = UnixDatagram::bind(&path).unwrap();
        sd_notify_to(path.to_str().unwrap(), "WATCHDOG=1");
        let mut buf = [0u8; 100];
        let n = server.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"WATCHDOG=1");
    }

    #[test]
    fn sd_notify_without_a_socket_is_a_noop() {
        sd_notify_to("/nonexistent/notify", "READY=1"); // must not panic
    }

    #[test]
    fn state_dir_precedence() {
        // Environment variables are process-wide: keep this the only test that touches it.
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("MATRIX_STATE_DIR", dir.path());
        assert_eq!(state_dir(), dir.path());
        std::env::remove_var("MATRIX_STATE_DIR");
        assert_eq!(state_dir().file_name().unwrap(), "matrix-display");
    }

    #[test]
    fn frame_stats_summary() {
        let mut st = FrameStats::default();
        st.record(10.0, false);
        st.record(30.0, true);
        let s = st.summary();
        assert!(s.contains("frames=2") && s.contains("avg=20.0ms") && s.contains("overruns=1"), "{s}");
    }

    #[test]
    fn system_health_stops_calling_vcgencmd_once_unreadable() {
        let calls = Cell::new(0);
        let readable = AtomicBool::new(true);
        let empty = || {
            calls.set(calls.get() + 1);
            Vcgencmd::Output(String::new())
        };
        system_health_with(&readable, &empty);
        system_health_with(&readable, &empty);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn system_health_retries_vcgencmd_after_a_timeout() {
        let calls = Cell::new(0);
        let readable = AtomicBool::new(true);
        let flaky = || {
            calls.set(calls.get() + 1);
            if calls.get() == 1 { Vcgencmd::Timeout } else { Vcgencmd::Output("throttled=0x0".into()) }
        };
        system_health_with(&readable, &flaky);
        assert!(system_health_with(&readable, &flaky).contains("throttled=0x0"));
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn build_state_uses_the_forecast_location_time_zone() {
        let h = harness(None, 15);
        let st = h.runner.build_state();
        assert_eq!(st.now.offset(), NOON.offset());
    }
}
