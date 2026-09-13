//! CLI-only config: `~/.config/agentero/config.toml` (isolated from GUI).

use crate::error::CliError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_vault: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translator_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known_vaults: Vec<String>,
}

pub fn config_path() -> Result<PathBuf, CliError> {
    // Prefer XDG-style `~/.config/agentero` (docs/development/cli.md) even on macOS,
    // so agents/scripts share one predictable path across platforms.
    let base = dirs::home_dir()
        .map(|h| h.join(".config"))
        .or_else(dirs::config_dir)
        .ok_or_else(|| CliError::message("cannot resolve user config directory (~/.config)"))?;
    Ok(base.join("agentero").join("config.toml"))
}

/// Legacy known-vaults note path; migrated into `config.toml` on load.
fn legacy_known_vaults_path() -> Result<PathBuf, CliError> {
    let base = dirs::home_dir()
        .map(|h| h.join(".config"))
        .or_else(dirs::config_dir)
        .ok_or_else(|| CliError::message("cannot resolve user config directory (~/.config)"))?;
    Ok(base.join("agentero").join("vaults.md"))
}

/// Read legacy `vaults.md` entries, if present.
fn read_legacy_known_vaults() -> Vec<String> {
    let Ok(path) = legacy_known_vaults_path() else {
        return Vec::new();
    };
    if !path.is_file() {
        return Vec::new();
    }
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let trimmed = line
            .trim_start_matches('-')
            .trim_start_matches('*')
            .trim_start_matches(' ')
            .to_string();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.clone()) {
            out.push(trimmed);
        }
    }
    out
}

/// Read all known vault absolute paths, deduplicated and in original order.
pub fn list_known_vaults() -> Result<Vec<String>, CliError> {
    let cfg = load()?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for v in &cfg.known_vaults {
        if seen.insert(v.clone()) {
            out.push(v.clone());
        }
    }
    Ok(out)
}

/// Append a vault path to `known_vaults` if it is not already present.
pub fn record_vault(path: &Path) -> Result<(), CliError> {
    let path = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut cfg = load()?;
    if cfg.known_vaults.iter().any(|p| p == &path) {
        return Ok(());
    }
    cfg.known_vaults.push(path);
    save(&cfg)?;
    Ok(())
}

pub fn load() -> Result<CliConfig, CliError> {
    let path = config_path()?;
    let mut cfg = if !path.is_file() {
        CliConfig::default()
    } else {
        let text = fs::read_to_string(&path)?;
        toml::from_str(&text)
            .map_err(|e| CliError::message(format!("parse config {}: {e}", path.display())))?
    };

    // One-time migration from legacy `vaults.md` into `config.toml`.
    let legacy = read_legacy_known_vaults();
    if !legacy.is_empty() {
        let mut seen: std::collections::HashSet<_> = cfg.known_vaults.iter().cloned().collect();
        for v in legacy {
            if seen.insert(v.clone()) {
                cfg.known_vaults.push(v);
            }
        }
        let _ = fs::remove_file(legacy_known_vaults_path()?);
        save(&cfg)?;
    }

    Ok(cfg)
}

pub fn save(cfg: &CliConfig) -> Result<PathBuf, CliError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(cfg)
        .map_err(|e| CliError::message(format!("serialize config: {e}")))?;
    fs::write(&path, text)?;
    Ok(path)
}
