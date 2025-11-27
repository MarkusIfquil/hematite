//! 
//! This module provides a helper for managing keypresses, allowing easy conversion between keycodes and keysyms.
//! `HotkeyAction`s force hotkeys to only implement the provided functions.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use x11rb::{
    connection::Connection,
    errors::ReplyOrIdError,
    protocol::xproto::{ConnectionExt as _, KeyButMask, KeyPressEvent, ModMask},
};
use xkeysym::{KeyCode, Keysym};

use crate::config::Config;
#[derive(Debug, Clone, Serialize, Deserialize)]
/// The possible actions a hotkey could activate.
pub enum HotkeyAction {
    /// Spawns the specified command.
    Spawn(String),
    /// Closes the currently focused window (if it exists).
    ExitFocusedWindow,
    /// Switches the active tag to the specified one.
    SwitchTag(usize),
    /// Moves the currently focused window to the specified tag.
    MoveWindow(usize),
    /// Changes the ratio between the `Master` and `Stack` groups by the specified amount.
    ChangeRatio(f32),
    /// Changes the window focus by the specified change.
    NextFocus(i16),
    /// Changes the active tag by the specified change.
    NextTag(i16),
    /// Swaps the focused window with the `Master` window.
    SwapMaster,
}

#[derive(Debug)]
/// Represents a hotkey.
pub struct Hotkey {
    /// The action a hotkey should activate
    action: HotkeyAction,
    /// This represents the codes of the pressed modifier buttons (e.g. CONTROL or SHIFT)
    mask: KeyButMask,
    /// The number associated with the key
    pub code: KeyCode,
    /// A key's internal name (e.g. `XK_ENTER`)
    _sym: Keysym,
    /// Contains the various pressed modifier buttons
    pub modifier: ModMask,
}

/// A helper for managing keypresses.
pub struct KeyHandler {
    /// A list of monitored hotkeys.
    pub hotkeys: Vec<Hotkey>,
    /// A map of keysyms and their respective keycodes. 
    _sym_code: HashMap<Keysym, KeyCode>,
}

impl KeyHandler {
    /// Creates a new handler.
    /// 
    /// A keyboard map is created based on the minimum and maximum keycodes, with keysyms being created with the xkeysym crate.
    /// 
    /// The hotkeys defined in the config file are grabbed and stored.
    /// 
    /// # Errors
    /// May return an error if the hotkeys are invalid.
    /// 
    /// # Panics
    /// 
    pub fn new(conn: &impl Connection, config: &Config) -> Result<Self, ReplyOrIdError> {
        //get min-max code
        let min = conn.setup().min_keycode;
        let max = conn.setup().max_keycode;

        //get mapping
        let mapping = conn
            .get_keyboard_mapping(min, max - min + 1)?
            .reply()?;

        //get sym-code pairings
        let sym_code: HashMap<Keysym, KeyCode> = (min..=max)
            .filter_map(|x| {
                xkeysym::keysym(
                    x.into(),
                    0,
                    min.into(),
                    mapping.keysyms_per_keycode,
                    mapping.keysyms.as_slice(),
                )
                .map(|s| (s, KeyCode::new(x.into())))
            })
            .collect();

        //get config hotkeys
        let hotkeys: Vec<Hotkey> = config
            .hotkeys
            .iter()
            .cloned()
            .map(|c| {
                let modi = c
                    .modifiers
                    .split('|')
                    .map(|m| match m {
                        "CONTROL" => KeyButMask::CONTROL,
                        "SHIFT" => KeyButMask::SHIFT,
                        "MOD" => KeyButMask::MOD4,
                        _ => KeyButMask::default(),
                    })
                    .fold(KeyButMask::default(), |acc, m| acc | m);

                let sym = match c.key.as_str() {
                    "XK_Return" => Keysym::Return,
                    "XF86_MonBrightnessUp" => Keysym::XF86_MonBrightnessUp,
                    "XF86_MonBrightnessDown" => Keysym::XF86_MonBrightnessDown,
                    "XF86_AudioRaiseVolume" => Keysym::XF86_AudioRaiseVolume,
                    "XF86_AudioLowerVolume" => Keysym::XF86_AudioLowerVolume,
                    "XF86_AudioMute" => Keysym::XF86_AudioMute,
                    "XK_Left" => Keysym::Left,
                    "XK_Right" => Keysym::Right,
                    c => {
                        let ch = c.chars().next().unwrap_or_else(|| {
                            log::error!("BAD KEYSYM {c}");
                            char::default()
                        });
                        Keysym::from_char(ch)
                    }
                };

                Hotkey {
                    _sym: sym,
                    code: *sym_code.get(&sym).expect("expected sym to have code"),
                    mask: modi,
                    modifier: ModMask::from(modi.bits()),
                    action: c.action,
                }
            })
            .collect();

        Ok(Self {
            _sym_code: sym_code,
            hotkeys,
        })
    }

    /// Gets a hotkey based on its mask and code.
    fn get_registered_hotkey(&self, mask: KeyButMask, code_raw: u32) -> Option<&Hotkey> {
        self.hotkeys
            .iter()
            .find(|h| mask == h.mask && code_raw == h.code.raw())
    }

    /// Gets the hotkey and its associated action based on a `KeyPressEvent`.
    #[must_use] 
    pub fn get_action(&self, event: KeyPressEvent) -> Option<HotkeyAction> {
        self.get_registered_hotkey(event.state, u32::from(event.detail))
            .map(|h| h.action.clone())
    }
}
