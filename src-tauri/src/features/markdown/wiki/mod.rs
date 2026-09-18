//! Wiki link index, resolution, rename transactions, and vault doctor checks.
//!
//! The tauri-free body lives in `agentero_core::features::markdown::wiki`;
//! this module bridges it and keeps the desktop-only command shells.

pub use agentero_core::features::markdown::wiki::*;

#[cfg(feature = "desktop")]
pub mod commands;
