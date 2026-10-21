//! Platform-independent core of the radial launcher: configuration, text
//! rendering and the wheel itself (geometry, hit-testing, drawing).
//!
//! Nothing in here touches the OS, so it can be unit-tested and previewed on
//! any platform. The Windows glue (mouse hook, overlay window, tray icon)
//! lives in the binary crate.

pub mod config;
pub mod text;
pub mod wheel;
