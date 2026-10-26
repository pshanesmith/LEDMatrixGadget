//! Home status display for a 64x64 RGB LED matrix: date, a procedural weather scene, and
//! Home Assistant lock / light / gate status. See DESIGN.md.

pub mod compositor;
pub mod config;
pub mod data;
pub mod display;
pub mod forecast;
pub mod gfx;
pub mod hadiscover;
pub mod layout;
pub mod pyrandom;
pub mod runner;
pub mod samples;
pub mod scene;
pub mod setconfig;
pub mod state;
pub mod ui;
pub mod util;
