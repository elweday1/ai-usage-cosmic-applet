// SPDX-License-Identifier: GPL-3.0-only

use cosmic::cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry};
use serde::{Deserialize, Serialize};

/// cosmic-config application id; matches the desktop entry / app id.
pub const CONFIG_ID: &str = "co.osterberg.ClaudeUsage";
/// cosmic-config schema version.
pub const CONFIG_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope { Session, Weekly, Worst, Both }

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Style { ColorDot, FillBar, FillColor, Ring, RingColor, VBar }

/// How time-to-reset is surfaced on the panel (the popup always shows it).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResetDisplay {
    None,
    Text,
    Compact,
    Glow,
    DualRing,
    Track,
    /// A standalone vertical time bar beside the indicator; works with any style.
    TimeColumn,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thresholds { pub amber: f32, pub red: f32 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct Config {
    pub scope: Scope,
    pub style: Style,
    pub show_percent: bool,
    /// For ring styles: draw the percentage centred in the ring's hole (true) or
    /// beside it (false). Beside stays legible alongside the dual-ring time arc.
    pub percent_inside_ring: bool,
    pub reset_display: ResetDisplay,
    pub thresholds: Thresholds,
    pub stale_after: u64,
    /// Overrides the Claude history path; other providers are auto-detected.
    pub history_path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            scope: Scope::Worst,
            style: Style::ColorDot,
            show_percent: false,
            percent_inside_ring: true,
            reset_display: ResetDisplay::None,
            thresholds: Thresholds { amber: 0.50, red: 0.80 },
            stale_after: 600,
            history_path: None,
        }
    }
}

impl Config {
    /// Load persisted config from cosmic-config, falling back to defaults on
    /// any error (missing config directory, unset keys, parse failures).
    pub fn load() -> Config {
        match cosmic_config::Config::new(CONFIG_ID, CONFIG_VERSION) {
            Ok(handler) => Config::get_entry(&handler).unwrap_or_else(|(_errs, cfg)| cfg),
            Err(_) => Config::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert!(matches!(c.scope, Scope::Worst));
        assert!(matches!(c.style, Style::ColorDot));
        assert_eq!(c.show_percent, false);
        assert_eq!(c.percent_inside_ring, true);
        assert!(matches!(c.reset_display, ResetDisplay::None));
        assert_eq!(c.thresholds.amber, 0.50);
        assert_eq!(c.thresholds.red, 0.80);
        assert_eq!(c.stale_after, 600);
        assert!(c.history_path.is_none());
    }
}
