//!
//! Event handling and core logic module for handling X11 errors.
//!
//! This module is basically just for the `EventHandler` struct.

use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            ClientMessageEvent, ConfigureRequestEvent, EnterNotifyEvent, KeyPressEvent,
            MapRequestEvent, UnmapNotifyEvent,
        },
    },
};

use crate::{
    bar::BarPainter,
    connection::{
        ConnectionActionExt as _, ConnectionAtomExt as _, ConnectionHandler,
        ConnectionStateExt as _, Res,
    },
    keys::{HotkeyAction, KeyHandler},
    state::{StateHandler, WindowGroup, WindowState},
};

/// The main struct handling events.
/// This struct employs all the other handlers and uses their apis to change the state or do something with X11, handling all the required events for a window manager.
pub struct EventHandler<'connection, C: Connection> {
    /// A struct to manage the bar.
    pub bar: BarPainter,
    /// A struct to manage X11 related actions.
    pub conn: ConnectionHandler<'connection, C>,
    /// An api to help with keypresses.
    pub key: KeyHandler,
    /// A struct to change the state of windows.
    pub state: StateHandler,
}

impl<C: Connection> EventHandler<'_, C> {
    /// Handles X11 events related to managing windows.
    ///
    /// Currently, only mapping, unmapping, keypresses, entering a window, configure requests and messages are handled.
    ///
    /// # Errors
    /// Any inappropriate call to the X11 server will be bubbled up by this function.
    pub fn handle_event(&mut self, event: &Event) -> Res {
        match event {
            Event::MapRequest(event) => {
                self.handle_map_request(*event)?;
            }
            Event::UnmapNotify(event) => {
                self.handle_unmap_notify(*event)?;
            }
            Event::KeyPress(event) => {
                self.handle_keypress(*event)?;
            }
            Event::EnterNotify(event) => {
                self.handle_enter(*event)?;
            }
            Event::ConfigureRequest(event) => {
                self.handle_config(*event)?;
            }
            Event::ClientMessage(event) => {
                self.handle_client_message(*event)?;
            }
            _ => (),
        }
        Ok(())
    }

    /// Handles a `MapRequestEvent`.
    ///
    /// Only maps unmapped windows. Adds the window (including frame) using a connection and adds the window to the state. Also refreshes the display.
    fn handle_map_request(&mut self, event: MapRequestEvent) -> Res {
        if self.state.get_window_state(event.window).is_some() {
            return Ok(());
        }

        log::trace!(
            "EVENT MAP window {} parent {} response {}",
            event.window,
            event.parent,
            event.response_type
        );

        let window = WindowState::new(event.window, self.conn.generate_id()?);

        self.conn.add_window(&window)?;
        self.state.add_window(window);
        self.refresh()
    }

    /// Handles an `UnmapNotifyEvent`.
    ///
    /// Only unmaps existing windows. Destroys the window and frame and removes it from the state. Also refreshes the display.
    fn handle_unmap_notify(&mut self, event: UnmapNotifyEvent) -> Res {
        let Some(window) = self.state.get_window_state(event.window) else {
            return Ok(());
        };
        log::trace!(
            "EVENT UNMAP window {} event {} from config {} response {}",
            event.window,
            event.event,
            event.from_configure,
            event.response_type
        );

        self.conn.destroy_frame_window(window)?;
        self.conn.net_update_client_list(
            &self.state.tags[self.state.active_tag]
                .windows
                .iter()
                .map(|w| w.window)
                .collect::<Vec<u32>>(),
        )?;

        self.bar.icons.remove(&window.window);
        self.state
            .get_mut_active_tag_windows()
            .retain(|w| w.window != event.window);

        self.state.set_tag_focus_to_master();
        self.refresh()
    }

    /// Handles a `KeyPressEvent`.
    ///
    /// Only parses keys with valid hotkey actions. The parsed action is also handled. Also refreshes the display.
    fn handle_keypress(&mut self, event: KeyPressEvent) -> Res {
        let Some(action) = self.key.get_action(event) else {
            return Ok(());
        };

        log::trace!(
            "EVENT KEYPRESS code {} sym {:?} action {:?}",
            event.detail,
            event.state,
            action
        );

        match action {
            HotkeyAction::SwitchTag(n) => {
                self.change_active_tag(n - 1)?;
            }
            HotkeyAction::MoveWindow(n) => {
                self.move_window(n - 1)?;
            }
            HotkeyAction::Spawn(command) => {
                crate::connection::spawn_command(&command);
            }
            HotkeyAction::ExitFocusedWindow => {
                let Some(focus) = self.state.get_focus() else {
                    return Ok(());
                };
                self.conn.kill_focus(focus)?;
            }
            HotkeyAction::ChangeRatio(change) => {
                self.state.tiling.ratio = (self.state.tiling.ratio + change).clamp(0.15, 0.85);
            }
            HotkeyAction::NextFocus(change) => {
                self.state.switch_focus_next(change);
            }
            HotkeyAction::NextTag(change) => {
                self.change_active_tag(
                    (self.state.active_tag as i16 + change).rem_euclid(9) as usize
                )?;
            }
            HotkeyAction::SwapMaster => {
                self.state.swap_master();
            }
        }
        self.refresh()?;
        Ok(())
    }

    /// Handles an `EnterNotfiyEvent`.
    ///
    /// Handles enters from window to window and window to root. Also refreshes the display.
    fn handle_enter(&mut self, event: EnterNotifyEvent) -> Res {
        log::trace!(
            "EVENT ENTER child {} detail {:?} event {}",
            event.child,
            event.detail,
            event.event
        );

        if let Some(w) = self.state.get_window_state(event.child) {
            self.state.tags[self.state.active_tag].focus = Some(w.window);
        }
        if let Some(w) = self.state.get_window_state(event.event) {
            self.state.tags[self.state.active_tag].focus = Some(w.window);
        }
        self.refresh()?;
        Ok(())
    }

    /// Handles a `ConfigureRequestEvent`.
    ///
    /// Only configures the window if it exists in the state.
    fn handle_config(&self, event: ConfigureRequestEvent) -> Res {
        if self.state.get_window_state(event.window).is_some() {
            self.conn.handle_config(event)?;
        }
        Ok(())
    }

    /// Handles a `ClientMessageEvent`.
    ///
    /// A client message is made up of a window and message data, usually containing atoms, meant to change the appearance or behaviour of a window.
    ///
    /// Currently only the fullscreen request message is handled.
    fn handle_client_message(&mut self, event: ClientMessageEvent) -> Res {
        let data = event.data.as_data32();

        log::trace!("got client data {data:?}");
        if data[1] == 0 {
            return Ok(());
        }

        let Ok(event_type) = self.conn.atoms.get_atom_name(event.type_) else {
            return Ok(());
        };

        let Ok(first_property) = self.conn.atoms.get_atom_name(data[1]) else {
            return Ok(());
        };

        log::trace!(
            "GOT CLIENT EVENT window {} atom {:?} first prop {:?}",
            event.window,
            event_type,
            first_property
        );

        if event_type.as_str() == "_NET_WM_STATE"
            && first_property.as_str() == "_NET_WM_STATE_FULLSCREEN"
        {
            let Some(state) = self.state.get_mut_window_state(event.window) else {
                return Ok(());
            };
            let window = state.window;
            match data[0] {
                0 => {
                    log::debug!("setting group of {window} to stack!");
                    state.group = WindowGroup::Stack;
                    self.conn.remove_fullscreen(state)?;
                    self.refresh()?;
                }
                1 => {
                    log::debug!("setting group of {window} to fullscreen!");
                    state.group = WindowGroup::Fullscreen;
                    self.conn.set_fullscreen(state)?;
                    self.refresh()?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Refreshes the state and status bar.
    ///
    /// This function does a laundry list of tasks:
    /// - Sets the focus using the focus set in state
    /// - Tiles windows using state
    /// - Configures every window in a tag
    /// - Draws the status bar
    /// - Logs the state
    fn refresh(&mut self) -> Res {
        self.refresh_focus()?;
        self.state.refresh();
        self.config_tag()?;
        self.draw_bar();
        self.state.log_state();
        Ok(())
    }

    /// Refreshes the displayed focus.
    ///
    /// If no window is focused the root window obtains the focus.
    fn refresh_focus(&self) -> Res {
        match self.state.tags[self.state.active_tag].focus {
            Some(w) => {
                let Some(window) = self.state.get_window_state(w) else {
                    return Ok(());
                };
                self.conn
                    .set_focus_window(self.state.get_active_tag_windows(), window)?;
            }
            None => {
                self.conn.set_focus_to_root()?;
            }
        }
        Ok(())
    }

    /// Switches the display from one tag to another, unmapping the old tag and mapping the new.
    ///
    /// Only switching between two different tags is permitted.
    fn change_active_tag(&mut self, tag: usize) -> Res {
        if self.state.active_tag == tag {
            log::debug!("tried switching to already active tag");
            return Ok(());
        }
        log::trace!("changing tag to {tag}");
        self.unmap_tag()?;
        self.state.active_tag = tag;
        self.map_tag()?;
        self.conn.net_update_active_desktop(tag as u32)?;
        Ok(())
    }

    /// Maps a tag's windows to the display.
    fn map_tag(&self) -> Res {
        self.state
            .get_active_tag_windows()
            .iter()
            .try_for_each(|w| self.conn.map(w))
    }

    /// Unmaps a tag's windows from the display.
    fn unmap_tag(&self) -> Res {
        self.state
            .get_active_tag_windows()
            .iter()
            .try_for_each(|w| self.conn.unmap(w))
    }

    /// Configures a tag's windows with their state.
    fn config_tag(&self) -> Res {
        self.state
            .get_active_tag_windows()
            .iter()
            .try_for_each(|w| self.conn.config_window_from_state(w))
    }

    /// Moves the focused window from one tag to another.
    ///
    /// Only moving to a different tag is permitted.
    fn move_window(&mut self, tag: usize) -> Res {
        if self.state.active_tag == tag {
            log::debug!("tried moving window to already active tag");
            return Ok(());
        }
        log::trace!("moving window to tag {tag}");

        let focus_window = self.conn.get_focus()?;

        let state = match self.state.get_window_state(focus_window) {
            Some(s) => *s,
            None => return Ok(()),
        };
        self.conn.unmap(&state)?;

        self.state.tags[tag].windows.push(state);
        self.state.tags[self.state.active_tag]
            .windows
            .retain(|w| w.window != focus_window);
        self.state.set_tag_focus_to_master();

        self.conn
            .net_update_window_desktop(focus_window, self.state.active_tag as u32)?;

        Ok(())
    }

    pub fn draw_bar(&mut self) {
        if let Err(error) = self
            .bar
            .draw_bar(&self.state, &self.conn, self.state.get_focus())
        {
            log::error!("{error}");
        }
    }
}
