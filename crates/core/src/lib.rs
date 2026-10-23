//! Platform-independent core of Radial Launcher: configuration, colours and themes, the
//! six wheel designs and their rendering, text abstraction and the icon cache file.
//!
//! The Windows-only helpers (shell icons, GDI text, autostart, IPC) live in [`win`].

pub mod color;
pub mod config;
pub mod iconcache;
pub mod text;
pub mod wheel;

#[cfg(windows)]
pub mod win;

/// Edge length (px) of the square icon bitmaps kept in the icon cache and in a session.
pub const ICON_PX: u32 = 96;
