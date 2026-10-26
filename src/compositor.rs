//! Frame composition. Regions are drawn by pluggable callables, so the scene and each
//! widget own their drawing without editing this file.

use anyhow::{bail, Result};

use crate::config::DisplayConfig;
use crate::gfx::Img;
use crate::layout::{self, Rect};
use crate::scene::SceneRenderer;
use crate::state::{AppState, DailyWeather, Dt};
use crate::ui::widgets;

/// draw(frame, rect, state, t): draw into `frame` only within `rect`; t is seconds.
pub type RegionDrawer = Box<dyn FnMut(&mut Img, Rect, &AppState, f64)>;

#[derive(Default)]
pub struct Compositor {
    pub debug_layout: bool,
    drawers: Vec<(String, RegionDrawer)>,
}

impl Compositor {
    pub fn new(debug_layout: bool) -> Self {
        Compositor { debug_layout, drawers: Vec::new() }
    }

    pub fn register(&mut self, region: &str, drawer: RegionDrawer) -> Result<()> {
        if layout::region(region).is_none() {
            let known: Vec<&str> = layout::REGIONS.iter().map(|(n, _)| *n).collect();
            bail!("unknown region {region:?}; known: {known:?}");
        }
        match self.drawers.iter_mut().find(|(n, _)| n == region) {
            Some(slot) => slot.1 = drawer,
            None => self.drawers.push((region.to_string(), drawer)),
        }
        Ok(())
    }

    pub fn has_drawer(&self, region: &str) -> bool {
        self.drawers.iter().any(|(n, _)| n == region)
    }

    pub fn render(&mut self, state: &AppState, t: f64) -> Img {
        let mut frame = Img::black(layout::WIDTH, layout::HEIGHT);
        for (name, rect) in layout::REGIONS {
            if let Some((_, drawer)) = self.drawers.iter_mut().find(|(n, _)| n == name) {
                drawer(&mut frame, rect, state, t);
            }
        }
        if self.debug_layout {
            for (_, r) in layout::REGIONS {
                frame.outline_rect(r.x, r.y, r.x2() - 1, r.y2() - 1, [60, 60, 60]);
            }
        }
        frame
    }
}

/// Compositor with the UI widgets and, when asked, the weather scene.
pub fn build_default_compositor(include_scene: bool, debug_layout: bool) -> Compositor {
    let mut comp = Compositor::new(debug_layout);
    for (name, drawer) in widgets::widgets() {
        comp.register(name, Box::new(drawer)).expect("widget regions exist");
    }
    if include_scene {
        let mut scene = SceneRenderer::new();
        comp.register("scene", Box::new(move |frame, rect, state, t| scene.draw(frame, rect, state, t)))
            .expect("scene region exists");
    }
    comp
}

/// Brightness percent: the configured default by day, about half from sunset to sunrise, never
/// above `max_brightness`. `daily` may be `None` (no weather yet).
pub fn brightness_for(now: Dt, daily: Option<&DailyWeather>, cfg: &DisplayConfig) -> u8 {
    let mut pct = cfg.brightness;
    if let Some(daily) = daily {
        let tz = now.timezone();
        let sunrise = daily.sunrise.with_timezone(&tz).time();
        let sunset = daily.sunset.with_timezone(&tz).time();
        let tod = now.time();
        if tod >= sunset || tod < sunrise {
            pct /= 2;
        }
    }
    pct.min(cfg.max_brightness).max(1)
}
