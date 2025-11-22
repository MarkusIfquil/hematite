use core::fmt;
use core::fmt::Debug;
use std::fmt::Write;
type Window = u32;
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WindowGroup {
    Master,
    Stack,
    Floating,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WindowState {
    pub window: Window,
    pub frame_window: Window,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub(crate) group: WindowGroup,
}

impl WindowState {
    pub fn new(window: Window, frame_window: Window) -> Self {
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

pub struct Tag {
    num: usize,
    pub focus: Option<u32>,
    pub windows: Vec<WindowState>,
}
impl Tag {
    fn new(tag: usize) -> Self {
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

pub struct TilingInfo {
    pub gap: u16,
    pub ratio: f32,
    pub width: u16,
    pub height: u16,
    pub bar_height: u16,
}

pub struct StateHandler {
    pub tags: Vec<Tag>,
    pub active_tag: usize,
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
    pub fn new(tiling: TilingInfo) -> Self {
        Self {
            tags: (0..=8).map(Tag::new).collect(),
            active_tag: 0,
            tiling,
        }
    }

    pub fn get_focus(&self) -> Option<u32> {
        self.tags[self.active_tag].focus
    }

    pub fn get_active_tag_windows(&self) -> &Vec<WindowState> {
        &self.tags[self.active_tag].windows
    }

    pub fn get_mut_active_tag_windows(&mut self) -> &mut Vec<WindowState> {
        &mut self.tags[self.active_tag].windows
    }

    pub fn get_window_state(&self, window: Window) -> Option<&WindowState> {
        self.tags[self.active_tag]
            .windows
            .iter()
            .find(|w| w.window == window || w.frame_window == window)
    }

    pub fn get_mut_window_state(&mut self, window: Window) -> Option<&mut WindowState> {
        self.tags[self.active_tag]
            .windows
            .iter_mut()
            .find(|w| w.window == window || w.frame_window == window)
    }

    pub fn add_window(&mut self, window: WindowState) {
        log::debug!("adding window to tag {}", self.active_tag);
        self.tags[self.active_tag].windows.push(window);
        self.tags[self.active_tag].focus = Some(window.window);
    }

    pub fn set_tag_focus_to_master(&mut self) {
        log::debug!("setting tag focus to master");
        self.tags[self.active_tag].focus =
            self.tags[self.active_tag].windows.last().map(|w| w.window);
    }

    pub fn set_last_master_others_stack(&mut self) {
        self.get_mut_active_tag_windows()
            .iter_mut()
            .filter(|w| w.group != WindowGroup::Floating)
            .for_each(|w| w.group = WindowGroup::Stack);

        if let Some(w) = self.get_mut_active_tag_windows().last_mut() {
            if w.group == WindowGroup::Floating {
                return;
            }
            w.group = WindowGroup::Master;
        }
    }

    pub fn tile_windows(&mut self) {
        log::debug!("tiling tag {}", self.active_tag);

        let (gap, ratio) = (self.tiling.gap, self.tiling.ratio);
        let (max_width, max_height) = (self.tiling.width, self.tiling.height);
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
            });
    }

    pub fn refresh(&mut self) {
        self.set_last_master_others_stack();
        self.tile_windows();
    }

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

    pub fn print_state(&self) {
        log::trace!("Manager state:\n{self}");
    }

    fn get_index_of_window(&self, window: Window) -> Option<usize> {
        self.tags[self.active_tag]
            .windows
            .iter()
            .position(|w| w.window == window || w.frame_window == window)
    }
}
