use crate::keys::HotkeyAction;
use serde::{Deserialize, Serialize};
use std::num::ParseIntError;
use x11rb::protocol::render::Color;

pub const SPACING: u32 = 10;
pub const RATIO: f32 = 0.5;
pub const BORDER_SIZE: u32 = 1;
pub const MAIN_COLOR: Color = Color {
    red: 4369,
    green: 4369,
    blue: 6939,
    alpha: 65535,
}; // #11111b
pub const SECONDARY_COLOR: Color = Color {
    red: 29812,
    green: 51143,
    blue: 60652,
    alpha: 65535,
}; // #74c7ec
pub const FONT: &str = "fixed";

fn hex_color_to_argb(hex: &str) -> Result<Color, ParseIntError> {
    Ok(Color {
        red: u16::from_str_radix(&hex[1..3], 16)? * 257,
        green: u16::from_str_radix(&hex[3..5], 16)? * 257,
        blue: u16::from_str_radix(&hex[5..7], 16)? * 257,
        alpha: 65535,
    })
}

#[derive(Clone)]
pub struct Config {
    pub spacing: u32,
    pub ratio: f32,
    pub border_size: u32,
    pub main_color: Color,
    pub secondary_color: Color,
    pub font: String,
    pub font_size: u32,
    pub hotkeys: Vec<HotkeyConfig>,
}

impl From<ConfigDeserialized> for Config {
    fn from(config: ConfigDeserialized) -> Self {
        let main_color = hex_color_to_argb(&config.colors.main_color).unwrap_or_else(|_| {
            log::debug!("BAD COLOR VALUE");
            MAIN_COLOR
        });
        let secondary_color =
            hex_color_to_argb(&config.colors.secondary_color).unwrap_or_else(|_| {
                log::debug!("BAD COLOR VALUE");
                SECONDARY_COLOR
            });

        Self {
            main_color,
            secondary_color,
            spacing: config.sizing.spacing.clamp(0, 1000),
            ratio: config.sizing.ratio.clamp(0.0, 1.0),
            border_size: config.sizing.border_size.clamp(0, 1000),
            font: config.font.name,
            font_size: config.font.size,
            hotkeys: config.hotkeys,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigDeserialized {
    sizing: Sizing,
    colors: Colors,
    font: Font,
    hotkeys: Vec<HotkeyConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Sizing {
    spacing: u32,
    ratio: f32,
    border_size: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Colors {
    main_color: String,
    secondary_color: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Font {
    name: String,
    size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub modifiers: String,
    pub key: String,
    pub action: HotkeyAction,
}

impl ConfigDeserialized {
    pub fn new() -> Self {
        let path =
            match xdg::BaseDirectories::with_prefix("hematite").place_config_file("config.toml") {
                Ok(p) => p,
                Err(e) => {
                    log::error!("cant create config file with error {e:?}, using default");
                    return Self::default();
                }
            };

        log::info!("loading config from {}", path.display());

        let config_str = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                log::info!("config not found {e:?}, serializing default");

                let Ok(serialized) = toml::to_string(&Self::default()) else {
                    log::error!("couldn't serialize config into file, using default");
                    return Self::default();
                };

                match std::fs::write(&path, serialized) {
                    Ok(()) => log::info!("created default config at {}", path.display()),
                    Err(_) => {
                        log::error!("couldn't write to file, using default");
                    }
                }

                return Self::default();
            }
        };

        match toml::from_str(&config_str) {
            Ok(d) => d,
            Err(e) => {
                log::error!("error parsing config {e:?}, using default");
                Self::default()
            }
        }
    }
    fn default() -> Self {
        log::info!("using default config");
        let mut hotkeys = vec![
            // terminal
            HotkeyConfig {
                modifiers: "CONTROL|MOD".to_string(),
                key: "XK_Return".to_string(),
                action: HotkeyAction::Spawn("alacritty".to_string()),
            },
            // browser
            HotkeyConfig {
                modifiers: "CONTROL|MOD".to_string(),
                key: "l".to_string(),
                action: HotkeyAction::Spawn("librewolf".to_string()),
            },
            // quit window
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "q".to_string(),
                action: HotkeyAction::ExitFocusedWindow,
            },
            // shutdown
            HotkeyConfig {
                modifiers: "CONTROL|MOD".to_string(),
                key: "q".to_string(),
                action: HotkeyAction::Spawn("killall hematite".to_string()),
            },
            // app starter
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "c".to_string(),
                action: HotkeyAction::Spawn("rofi -show drun".to_string()),
            },
            // screenshot
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "u".to_string(),
                action: HotkeyAction::Spawn(
                    "maim --select | xclip -selection clipboard -t image/png".to_string(),
                ),
            },
            // change ratio
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "h".to_string(),
                action: HotkeyAction::ChangeRatio(-0.05),
            },
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "j".to_string(),
                action: HotkeyAction::ChangeRatio(0.05),
            },
            // change focus
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "k".to_string(),
                action: HotkeyAction::NextFocus(1),
            },
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "l".to_string(),
                action: HotkeyAction::NextFocus(-1),
            },
            // change tag
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "XK_Left".to_string(),
                action: HotkeyAction::NextTag(-1),
            },
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "XK_Right".to_string(),
                action: HotkeyAction::NextTag(1),
            },
            // swap master
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "XK_Return".to_string(),
                action: HotkeyAction::SwapMaster,
            },
            //media
            HotkeyConfig {
                modifiers: String::new(),
                key: "XF86_AudioRaiseVolume".to_string(),
                action: HotkeyAction::Spawn("/usr/bin/pactl set-sink-volume 0 +5%".to_string()),
            },
            HotkeyConfig {
                modifiers: String::new(),
                key: "XF86_AudioLowerVolume".to_string(),
                action: HotkeyAction::Spawn("/usr/bin/pactl set-sink-volume 0 -5%".to_string()),
            },
            HotkeyConfig {
                modifiers: String::new(),
                key: "XF86_AudioMute".to_string(),
                action: HotkeyAction::Spawn("/usr/bin/pactl set-sink-mute 0 toggle".to_string()),
            },
            HotkeyConfig {
                modifiers: String::new(),
                key: "XF86_MonBrightnessUp".to_string(),
                action: HotkeyAction::Spawn("sudo light -A 5".to_string()),
            },
            HotkeyConfig {
                modifiers: String::new(),
                key: "XF86_MonBrightnessDown".to_string(),
                action: HotkeyAction::Spawn("sudo light -U 5".to_string()),
            },
        ];
        hotkeys.extend(
            // switch to tag
            (1..=9)
                .map(|x| HotkeyConfig {
                    modifiers: "MOD".to_string(),
                    key: x.to_string(),
                    action: HotkeyAction::SwitchTag(x),
                })
                // move window to tag
                .chain((1..=9).map(|x| HotkeyConfig {
                    modifiers: "MOD|SHIFT".to_string(),
                    key: x.to_string(),
                    action: HotkeyAction::MoveWindow(x),
                })),
        );

        Self {
            sizing: Sizing {
                spacing: SPACING,
                ratio: RATIO,
                border_size: BORDER_SIZE,
            },
            colors: Colors {
                main_color: String::from("#11111b"),
                secondary_color: String::from("#74c7ec"),
            },
            font: Font {
                name: FONT.to_owned(),
                size: 12,
            },
            hotkeys,
        }
    }
}
