//! Shared TUI module for CodeFlow interactive dashboards.
//!
//! Feature-gated behind `#[cfg(feature = "tui")]`. Provides:
//! - `data` — view-model structs and data fetching from DB/filesystem
//! - `theme` — color palette, unicode symbols, border styles, text presets
//! - `widgets` — reusable ratatui widgets (status badge, phase badge, etc.)

pub mod data;
pub mod duration;
pub mod theme;
pub mod widgets;
