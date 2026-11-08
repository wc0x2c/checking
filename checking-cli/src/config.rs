use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    #[serde(default)]
    pub laps_remaining: OverlayWindowConfig,
    #[serde(default)]
    pub fuel_to_add: OverlayWindowConfig,
    /// The strategy result: the pit lap, and the fuel saving target that
    /// removes one stop.
    #[serde(default)]
    pub strategy: OverlayWindowConfig,
    #[serde(default)]
    pub avg_laps_left: OverlayWindowConfig,
    #[serde(default)]
    pub push_laps_left: OverlayWindowConfig,
    #[serde(default)]
    pub low_laps_left: OverlayWindowConfig,
    #[serde(default)]
    pub max_laps_left: OverlayWindowConfig,
    #[serde(default)]
    pub relative: RelativeOverlayConfig,
    #[serde(default)]
    pub pedal: PedalOverlayConfig,
    #[serde(default)]
    pub radar_left: OverlayWindowConfig,
    #[serde(default)]
    pub radar_right: OverlayWindowConfig,
    #[serde(default)]
    pub radar: RadarOverlayConfig,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "default_extra_liters")]
    pub extra_liters: f32,
    #[serde(skip)]
    pub fuel_tank_pct: Option<f32>,
}

fn default_extra_liters() -> f32 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelativeOverlayConfig {
    #[serde(default)]
    pub window: OverlayWindowConfig,
    #[serde(default)]
    pub show_minimap: bool,
    #[serde(default)]
    pub minimap_mode: MinimapMode,
    #[serde(default = "default_num_positions")]
    pub num_positions: usize,
    #[serde(default)]
    pub buddy_list: Vec<String>,
    #[serde(default)]
    pub warning_list: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedalOverlayConfig {
    #[serde(default)]
    pub window: OverlayWindowConfig,
    #[serde(default = "default_pedal_history_seconds")]
    pub history_seconds: f32,
}

fn default_pedal_history_seconds() -> f32 {
    20.0
}

/// Settings for the radar indicators that both sides use. The window position
/// and size for each side are in `radar_left` and `radar_right`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadarOverlayConfig {
    /// Distance in metres in front of and behind the car that the strip shows.
    #[serde(default = "default_radar_range_m")]
    pub range_m: f32,
    /// Car length to use until the app learns the length for the car class.
    #[serde(default = "default_radar_fallback_car_length_m")]
    pub fallback_car_length_m: f32,
}

fn default_radar_range_m() -> f32 {
    8.0
}

fn default_radar_fallback_car_length_m() -> f32 {
    4.5
}

impl Default for RadarOverlayConfig {
    fn default() -> Self {
        Self {
            range_m: default_radar_range_m(),
            fallback_car_length_m: default_radar_fallback_car_length_m(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MinimapMode {
    Relative, // The player car stays in the center.
    Absolute, // The start/finish line stays in the center.
}

impl Default for MinimapMode {
    fn default() -> Self {
        MinimapMode::Relative
    }
}

fn default_num_positions() -> usize {
    7 // 3 ahead, the player and 3 behind
}

impl Default for RelativeOverlayConfig {
    fn default() -> Self {
        Self {
            window: OverlayWindowConfig {
                enabled: false,
                x: 750.0,
                y: 300.0,
                width: 400.0,
                height: 600.0,
            },
            show_minimap: true,
            minimap_mode: MinimapMode::Relative,
            num_positions: default_num_positions(),
            buddy_list: vec![],
            warning_list: vec![],
        }
    }
}

impl Default for PedalOverlayConfig {
    fn default() -> Self {
        Self {
            window: OverlayWindowConfig {
                enabled: false,
                x: 100.0,
                y: 500.0,
                width: 350.0,
                height: 350.0,
            },
            history_seconds: default_pedal_history_seconds(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayWindowConfig {
    pub enabled: bool,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            laps_remaining: OverlayWindowConfig {
                enabled: false,
                x: 100.0,
                y: 100.0,
                width: 235.0,
                height: 110.0,
            },
            fuel_to_add: OverlayWindowConfig {
                enabled: false,
                x: 400.0,
                y: 100.0,
                width: 300.0,
                height: 180.0,
            },
            strategy: OverlayWindowConfig {
                enabled: false,
                x: 400.0,
                y: 320.0,
                width: 260.0,
                height: 90.0,
            },
            avg_laps_left: OverlayWindowConfig {
                enabled: false,
                x: 750.0,
                y: 100.0,
                width: 140.0,
                height: 120.0,
            },
            push_laps_left: OverlayWindowConfig {
                enabled: false,
                x: 100.0,
                y: 300.0,
                width: 140.0,
                height: 120.0,
            },
            low_laps_left: OverlayWindowConfig {
                enabled: false,
                x: 400.0,
                y: 300.0,
                width: 140.0,
                height: 120.0,
            },
            max_laps_left: OverlayWindowConfig {
                enabled: false,
                x: 750.0,
                y: 300.0,
                width: 140.0,
                height: 120.0,
            },
            relative: RelativeOverlayConfig::default(),
            pedal: PedalOverlayConfig::default(),
            radar_left: OverlayWindowConfig {
                enabled: false,
                x: 50.0,
                y: 400.0,
                width: 120.0,
                height: 120.0,
            },
            radar_right: OverlayWindowConfig {
                enabled: false,
                x: 1750.0,
                y: 400.0,
                width: 120.0,
                height: 120.0,
            },
            radar: RadarOverlayConfig::default(),
            locked: true, // The windows cannot move until you unlock them.
            extra_liters: default_extra_liters(),
            fuel_tank_pct: None,
        }
    }
}

impl Default for OverlayWindowConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 150.0,
        }
    }
}

impl OverlayConfig {
    pub fn config_path() -> PathBuf {
        // Windows: C:\Users\<username>\AppData\Roaming\checking\overlay_config.toml
        // If APPDATA is not set, use the current directory.
        let config_dir = if cfg!(target_os = "windows") {
            std::env::var("APPDATA")
                .map(|appdata| PathBuf::from(appdata).join("checking"))
                .unwrap_or_else(|_| PathBuf::from(".").join("checking"))
        } else {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("checking")
        };

        fs::create_dir_all(&config_dir).ok();

        config_dir.join("overlay_config.toml")
    }

    pub fn load() -> Self {
        let path = Self::config_path();

        if let Ok(contents) = fs::read_to_string(&path) {
            toml::from_str(&contents).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path();
        let contents = toml::to_string_pretty(self)?;
        fs::write(path, contents)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config file from before the radar settings. All overlay sections must
    /// load, with the default radar settings. Thus an upgrade does not reset
    /// the window positions that the user saved.
    const LEGACY_CONFIG: &str = r#"
locked = true

[laps_remaining]
enabled = false
x = 100.0
y = 100.0
width = 250.0
height = 150.0

[radar_left]
enabled = true
x = 50.0
y = 400.0
width = 120.0
height = 120.0

[radar_right]
enabled = true
x = 1750.0
y = 400.0
width = 120.0
height = 120.0
"#;

    #[test]
    fn legacy_config_without_radar_key_still_loads() {
        let cfg: OverlayConfig = toml::from_str(LEGACY_CONFIG).expect("legacy config should parse");

        // The saved values do not change.
        assert!(cfg.locked);
        assert_eq!(cfg.laps_remaining.x, 100.0);
        assert!(cfg.radar_left.enabled);
        assert_eq!(cfg.radar_right.x, 1750.0);

        // The new settings get their default values, and the parse does not fail.
        assert_eq!(cfg.radar.range_m, default_radar_range_m());
        assert_eq!(
            cfg.radar.fallback_car_length_m,
            default_radar_fallback_car_length_m()
        );
    }

    #[test]
    fn radar_config_round_trips() {
        let mut cfg = OverlayConfig::default();
        cfg.radar.range_m = 10.0;
        cfg.radar.fallback_car_length_m = 5.2;

        let text = toml::to_string_pretty(&cfg).expect("serialize");
        let parsed: OverlayConfig = toml::from_str(&text).expect("deserialize");

        assert_eq!(parsed.radar.range_m, 10.0);
        assert_eq!(parsed.radar.fallback_car_length_m, 5.2);
    }
}
