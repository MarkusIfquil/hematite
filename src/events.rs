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
    actions::{ConnectionHandler, Res},
    keys::{HotkeyAction, KeyHandler},
    state::{StateHandler, WindowGroup, WindowState},
};

pub struct EventHandler<'a, C: Connection> {
    pub conn: &'a ConnectionHandler<'a, C>,
    pub state: StateHandler,
    pub key: KeyHandler,
}

impl<C: Connection> EventHandler<'_, C> {
    pub fn handle_event(&mut self, event: &Event) -> Res {
        match event {
            Event::MapRequest(e) => {
                self.handle_map_request(*e)?;
            }
            Event::UnmapNotify(e) => {
                self.handle_unmap_notify(*e)?;
            }
            Event::KeyPress(e) => {
                self.handle_keypress(*e)?;
            }
            Event::EnterNotify(e) => {
                self.handle_enter(*e)?;
            }
            Event::ConfigureRequest(e) => {
                self.handle_config(*e)?;
            }
            Event::ClientMessage(e) => {
                self.handle_client_message(*e)?;
            }
            _ => (),
        }
        Ok(())
    }

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

        let window = WindowState::new(event.window, self.conn.conn.generate_id()?);

        self.conn.add_window(&window)?;
        self.state.add_window(window);
        self.refresh()
    }

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

        self.conn.destroy_window(window)?;
        self.conn.update_client_list(&self.state)?;

        self.state
            .get_mut_active_tag_windows()
            .retain(|w| w.window != event.window);
        self.state.set_tag_focus_to_master();
        self.refresh()
    }

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
                crate::actions::spawn_command(&command);
            }
            HotkeyAction::ExitFocusedWindow => {
                let Some(focus) = self.state.get_focus() else { return Ok(()) };
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

    fn handle_config(&self, event: ConfigureRequestEvent) -> Res {
        if self.state.get_window_state(event.window).is_some() {
            self.conn.handle_config(event)?;
        }
        Ok(())
    }

    fn handle_client_message(&mut self, event: ClientMessageEvent) -> Res {
        let data = event.data.as_data32();

        log::trace!("got client data {data:?}");
        if data[1] == 0 {
            return Ok(());
        }

        let event_type = self.conn.get_atom_name(event.type_)?;

        let first_property = self.conn.get_atom_name(data[1])?;

        log::trace!(
            "GOT CLIENT EVENT window {} atom {:?} first prop {:?}",
            event.window,
            event_type,
            first_property
        );

        if event_type.as_str() == "_NET_WM_STATE"
            && first_property.as_str() == "_NET_WM_STATE_FULLSCREEN"
        {
            let Some(state) = self.state.get_mut_window_state(event.window) else { return Ok(()) };
            let window = state.window;
            match data[0] {
                0 => {
                    state.group = WindowGroup::Stack;
                    self.conn.remove_atom_prop(window, "_NET_WM_STATE")?;
                    self.refresh()?;
                }
                1 => {
                    state.group = WindowGroup::Floating;
                    state.x = 0;
                    state.y = 0;
                    state.width = self.conn.screen.width_in_pixels;
                    state.height = self.conn.screen.height_in_pixels;
                    self.conn.set_fullscreen(state)?;
                    self.refresh()?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn refresh(&mut self) -> Res {
        self.refresh_focus()?;
        self.state.refresh();
        self.config_tag()?;
        self.conn.refresh(&self.state)?;
        self.state.print_state();
        Ok(())
    }

    fn refresh_focus(&self) -> Res {
        match self.state.tags[self.state.active_tag].focus {
            Some(w) => {
                let Some(window) = self.state.get_window_state(w) else { return Ok(()) };
                self.conn
                    .set_focus_window(self.state.get_active_tag_windows(), window)?;
            }
            None => {
                self.conn.set_focus_to_root()?;
            }
        }
        Ok(())
    }

    fn change_active_tag(&mut self, tag: usize) -> Res {
        if self.state.active_tag == tag {
            log::debug!("tried switching to already active tag");
            return Ok(());
        }
        log::trace!("changing tag to {tag}");
        self.unmap_tag()?;
        self.state.active_tag = tag;
        self.map_tag()?;
        self.conn.update_active_desktop(tag as u32)?;
        Ok(())
    }

    fn map_tag(&self) -> Res {
        self.state
            .get_active_tag_windows()
            .iter()
            .try_for_each(|w| self.conn.map(w))
    }

    fn unmap_tag(&self) -> Res {
        self.state
            .get_active_tag_windows()
            .iter()
            .try_for_each(|w| self.conn.unmap(w))
    }

    fn config_tag(&self) -> Res {
        self.state
            .get_active_tag_windows()
            .iter()
            .try_for_each(|w| self.conn.config_window_from_state(w))
    }

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
            .update_window_desktop(focus_window, self.state.active_tag as u32)?;

        Ok(())
    }
}
