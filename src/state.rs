//!
//! This module manages the state using the `StateHandler` struct. It tiles windows and manages the windows' size and position.

use core::fmt;
use core::fmt::Debug;
use core::fmt::Write as _;

use x11rb::protocol::xproto::Window;
#[derive(Clone, Copy, PartialEq, Debug)]
/// An enum to track which group a window should be in, affecting how they're tiled.
pub enum WindowGroup {
    /// Master windows receive the biggest share of the work area and are not affected by how many windows there are.
    Master,
    /// Stack windows are stacked on top of each other, sharing their own space.
    Stack,
    /// Floating windows do not obey tiling rules and can be dragged around.
    Floating,
    /// Fullscreen windows are maximised to the screen and hide other windows.
    Fullscreen,
}

#[derive(Clone, Copy, PartialEq, Debug)]
/// The geometry, group and ids of a window.
pub struct WindowState {
    /// An X11 id referring to a window resource. This id is used to represent the window.
    pub window: Window,
    /// An X11 id referring to a frame window. A frame window wraps the regular window and is a parent of it, allowing the window to be configured.
    pub frame_window: Window,
    /// The X coordinate of the window.
    pub x: i16,
    /// The Y coordinate of the window.
    pub y: i16,
    /// The width in pixels of the window.
    pub width: u16,
    /// The height in pixels of the window.
    pub height: u16,
    /// The group of the window.
    pub group: WindowGroup,
}

impl WindowState {
    /// Creates a new window with base dimensions (0,0,100,100).
    ///
    /// New windows are immediately tiled, so these base values do not matter. New windows are `Stack` by default, but this can be changed immediately by the tiling logic.
    #[must_use]
    pub const fn new(window: Window, frame_window: Window) -> Self {
        Self {
            window,
            frame_window,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            group: WindowGroup::Stack,
        }
    }
}

impl fmt::Display for WindowState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "id {} fid {} x {} y {} w {} h {} g {:?}",
            self.window, self.frame_window, self.x, self.y, self.width, self.height, self.group
        )
    }
}
/// A virtual desktop containing windows and the id of the focused window.
///
/// Tags are numbered from 1-9, though this will be configurable in the future.
pub struct Tag {
    /// The index of a tag.
    num: usize,
    /// The focused window's id. Is `None` if no window is focused.
    pub focus: Option<u32>,
    /// The window states pertaining to the tag.
    pub windows: Vec<WindowState>,
}
impl Tag {
    /// Creates a new empty tag.
    const fn new(tag: usize) -> Self {
        Self {
            num: tag,
            focus: None,
            windows: Vec::new(),
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tag {} | focus {:?} | windows:\n{}",
            self.num,
            self.focus,
            self.windows.iter().fold(String::new(), |mut acc, w| {
                let _ = writeln!(acc, "{w}");
                acc
            })
        )
    }
}
/// Parameters that help with tiling windows. Values are obtained from configuration.
pub struct TilingInfo {
    /// The gap between a window and its surrounding edges.
    pub gap: u16,
    /// The ratio between the master and stack groups. The higher the number, the more space is allocated for the master group.
    pub ratio: f32,
    /// The maximum possible width to be allocated. This is usually the width of the screen.
    pub max_width: u16,
    /// The maximum possible height to be allocated. This is usually the height of the screen.
    pub max_height: u16,
    /// The height of the status bar.
    pub bar_height: u16,
}

/// A manager for window and tag states. Tiles windows and provides methods to manipulate the state.
pub struct StateHandler {
    /// Tags pertaining to the manager.
    pub tags: Vec<Tag>,
    /// The currently displayed tag's index.
    pub active_tag: usize,
    /// Information that helps with tiling.
    pub tiling: TilingInfo,
}

impl fmt::Display for StateHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "active tag {}\ntags:\n{}",
            self.tags
                .iter()
                .filter(|t| !t.windows.is_empty())
                .fold(String::new(), |mut acc, t| {
                    let _ = write!(acc, "{t}");
                    acc
                }),
            self.active_tag
        )
    }
}

impl StateHandler {
    /// Creates a new handler.
    ///
    /// Creates new empty tags and sets the active tag to be the first one.
    pub fn new(tiling: TilingInfo) -> Self {
        Self {
            tags: (0..=8).map(Tag::new).collect(),
            active_tag: 0,
            tiling,
        }
    }

    /// Gets the active tag's currently focused window. Returns `None` if no window is focused.
    #[must_use]
    pub fn get_focus(&self) -> Option<u32> {
        self.tags[self.active_tag].focus
    }

    /// Gets a reference to the window states of the currently active tag.
    #[must_use]
    pub fn get_active_tag_windows(&self) -> &Vec<WindowState> {
        &self.tags[self.active_tag].windows
    }

    /// Gets a mutable reference to the window states of the currently active tag.
    pub fn get_mut_active_tag_windows(&mut self) -> &mut Vec<WindowState> {
        &mut self.tags[self.active_tag].windows
    }

    /// Gets a reference to the state of a window based on that window's id. Returns `None` if no window exists.
    #[must_use]
    pub fn get_window_state(&self, window: Window) -> Option<&WindowState> {
        self.tags[self.active_tag]
            .windows
            .iter()
            .find(|w| w.window == window || w.frame_window == window)
    }

    /// Gets a mutable reference to the state of a window based on that window's id. Return `None` if no window exists.
    pub fn get_mut_window_state(&mut self, window: Window) -> Option<&mut WindowState> {
        self.tags[self.active_tag]
            .windows
            .iter_mut()
            .find(|w| w.window == window || w.frame_window == window)
    }

    /// Adds the window and its state to the currently active tag, and sets it to be the focused window.
    pub fn add_window(&mut self, window: WindowState) {
        log::debug!("adding window to tag {}", self.active_tag);
        self.tags[self.active_tag].windows.push(window);
        self.tags[self.active_tag].focus = Some(window.window);
    }

    /// Sets the tag's master window to be the focused window.
    pub fn set_tag_focus_to_master(&mut self) {
        log::debug!("setting tag focus to master");
        self.tags[self.active_tag].focus =
            self.tags[self.active_tag].windows.last().map(|w| w.window);
    }

    /// Sets all windows in a tag that are not in the `Floating` group to be `Stack`, then sets the last non floating window to `Master`.
    pub fn set_last_master_others_stack(&mut self) {
        self.get_mut_active_tag_windows()
            .iter_mut()
            .filter(|w| w.group != WindowGroup::Floating && w.group != WindowGroup::Fullscreen)
            .for_each(|w| w.group = WindowGroup::Stack);

        if let Some(w) = self.get_mut_active_tag_windows().last_mut() {
            if w.group == WindowGroup::Floating || w.group == WindowGroup::Fullscreen {
                return;
            }
            w.group = WindowGroup::Master;
        }
    }

    /// Tiles the windows of a tag, changing their position and size.
    ///
    /// Tiling is based around the dividing line that separates `Master` and `Stack` windows. The tiling ratio determines where this line sits.
    ///
    /// The `Master` window occupies the entirety of its side of the dividing line.
    ///
    /// `Stack` windows are in a "stack group", where they are positioned top to bottom according to where they are in the list. Their size depends on how many windows there are, with the whole Stack group taking the entire space of its side of the dividing line.
    ///
    /// `Floating` windows do not obey stacking rules are are drawn on top of all other windows (except `Fullscreen` windows).
    ///
    /// `Fullscreen` windows take up the entire screen and hide all other windows.
    pub fn tile_windows(&mut self) {
        log::debug!("tiling tag {}", self.active_tag);

        let (gap, ratio) = (self.tiling.gap, self.tiling.ratio);
        let (max_width, max_height) = (self.tiling.max_width, self.tiling.max_height);
        let bar_height = self.tiling.bar_height;

        let stack_count = self.get_active_tag_windows().len().clamp(1, 100) - 1;

        self.get_mut_active_tag_windows()
            .iter_mut()
            .enumerate()
            .for_each(|(i, w)| match w.group {
                WindowGroup::Master => {
                    w.x = gap as i16;
                    w.y = gap as i16 + bar_height as i16;
                    w.width = if stack_count == 0 {
                        max_width - gap * 2
                    } else {
                        f32::from(max_width).mul_add(1.0 - ratio, -(f32::from(gap) * 2.0)) as u16
                    };
                    w.height = max_height - gap * 2 - bar_height;
                }
                WindowGroup::Stack => {
                    w.x = (f32::from(max_width) * (1.0 - ratio)) as i16;
                    w.y = if i == 0 {
                        (i * (max_height as usize / stack_count) + gap as usize) as i16
                            + bar_height as i16
                    } else {
                        (i * (max_height as usize / stack_count)) as i16
                    };
                    w.width = (f32::from(max_width) * ratio) as u16 - gap;

                    w.height = if i == 0 {
                        (max_height as usize / stack_count) as u16 - gap * 2 - bar_height
                    } else {
                        (max_height as usize / stack_count) as u16 - gap
                    };
                }
                WindowGroup::Floating => (),
                WindowGroup::Fullscreen => {
                    w.x = 0;
                    w.y = 0;
                    w.width = max_width;
                    w.height = max_height;
                }
            });
    }

    /// Sets the window groups and tiles the windows of the active tag.
    pub fn refresh(&mut self) {
        self.set_last_master_others_stack();
        self.tile_windows();
    }

    /// Swaps the currently focused window with the `Master` window, changing their positions and sizes.
    ///
    /// If the focused window is the `Master` window, then nothing changes.
    pub fn swap_master(&mut self) {
        let Some(focus_window) = self.tags[self.active_tag].focus else {
            return;
        };
        let len = self.tags[self.active_tag].windows.len();
        let mut master = self.tags[self.active_tag].windows[len - 1].window;
        if master == focus_window && len > 1 {
            master = self.tags[self.active_tag].windows[len - 2].window;
        }
        let Some(index_f) = self.get_index_of_window(focus_window) else {
            return;
        };
        let Some(index_m) = self.get_index_of_window(master) else {
            return;
        };
        self.tags[self.active_tag].windows.swap(index_f, index_m);
    }

    /// Changes the focused window to be the next one in the list, with change denoting the jump in index. If negative, the focus is changed in the opposite order.
    pub fn switch_focus_next(&mut self, change: i16) {
        let Some(focus_window) = self.tags[self.active_tag].focus else {
            return;
        };
        let Some(focus_index) = self
            .get_active_tag_windows()
            .iter()
            .position(|w| w.window == focus_window)
        else {
            return;
        };
        let focus_index = focus_index as i16 + change;
        let focus_index = focus_index.rem_euclid(self.get_active_tag_windows().len() as i16);
        self.tags[self.active_tag].focus =
            Some(self.get_active_tag_windows()[focus_index as usize].window);
    }

    /// Logs the state of the manager:
    /// - non empty tags
    /// - focused windows
    /// - mapped windows
    pub fn log_state(&self) {
        log::trace!("Manager state:\n{self}");
    }

    /// Gets the index of a window in the active tag based on its id.
    ///
    /// Returns `None` if no such window exists.
    fn get_index_of_window(&self, window: Window) -> Option<usize> {
        self.tags[self.active_tag]
            .windows
            .iter()
            .position(|w| w.window == window || w.frame_window == window)
    }
}
