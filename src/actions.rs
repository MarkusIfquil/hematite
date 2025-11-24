use std::process::Command;
use std::process::exit;

use x11rb::protocol::render::Color;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::xproto::Pixmap;
use x11rb::wrapper::ConnectionExt as OtherConnectionExt;
use x11rb::{
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME,
    connection::Connection,
    cursor,
    errors::{ReplyError, ReplyOrIdError},
    protocol::{
        ErrorKind,
        xproto::{
            AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureRequestEvent,
            ConfigureWindowAux, CreateGCAux, CreateWindowAux, EventMask, Gcontext, GrabMode,
            ImageFormat, InputFocus, PropMode, Screen, SetMode, Window, WindowClass,
        },
    },
    resource_manager,
};

use crate::atoms::Atoms;
use crate::{
    config::Config,
    keys::KeyHandler,
    state::{StateHandler, WindowGroup, WindowState},
};

pub type Res = Result<(), ReplyOrIdError>;
pub type Id = u32;

pub struct Colors {
    pub main: Id,
    pub secondary: Id,
}

pub struct ConnectionHandler<'a, C: Connection> {
    pub conn: &'a C,
    pub screen: &'a Screen,
    screen_num: usize,
    pub atoms: Atoms<'a, C>,
    pub config: Config,
    pub colors: Colors,
}

impl<'a, C: Connection> ConnectionHandler<'a, C> {
    pub fn new(conn: &'a C, screen_num: usize, config: &Config) -> Result<Self, ReplyOrIdError> {
        let screen = &conn.setup().roots[screen_num];
        become_window_manager(conn, screen.root)?;

        log::trace!("screen num {screen_num} root {}", screen.root);

        let atoms = Atoms::new(conn, screen)?;

        let main_color = get_color_id(conn, screen, config.main_color)?;
        let secondary_color = get_color_id(conn, screen, config.secondary_color)?;

        let handler = ConnectionHandler {
            conn,
            screen,
            screen_num,
            atoms,
            config: config.clone(),
            colors: Colors {
                main: main_color,
                secondary: secondary_color,
            },
        };

        handler.grab_keys(&KeyHandler::new(conn, config)?)?;
        handler.set_cursor()?;
        handler.add_heartbeat_window()?;
        Ok(handler)
    }

    pub fn map(&self, window: &WindowState) -> Res {
        log::trace!("handling map of {}", window.window);
        self.conn.map_window(window.frame_window)?;
        self.conn.map_window(window.window)?;
        Ok(())
    }

    pub fn unmap(&self, window: &WindowState) -> Res {
        log::trace!("handling unmap of {}", window.window);
        self.conn.unmap_window(window.window)?;
        self.conn.unmap_window(window.frame_window)?;
        Ok(())
    }

    pub fn handle_config(&self, event: ConfigureRequestEvent) -> Res {
        log::trace!(
            "EVENT CONFIG w {} x {} y {} w {} self.bar.height {}",
            event.window,
            event.x,
            event.y,
            event.width,
            event.height
        );
        let aux = ConfigureWindowAux::from_configure_request(&event);
        self.conn.configure_window(event.window, &aux)?;
        Ok(())
    }

    pub fn add_window(&self, window: &WindowState) -> Res {
        log::trace!("creating frame of {}", window.window);
        self.conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window.frame_window,
            self.screen.root,
            window.x,
            window.y,
            window.width,
            window.height,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .event_mask(
                    EventMask::KEY_PRESS
                        | EventMask::SUBSTRUCTURE_NOTIFY
                        | EventMask::ENTER_WINDOW
                        | EventMask::PROPERTY_CHANGE,
                )
                .background_pixel(self.colors.main)
                .border_pixel(self.colors.secondary),
        )?;

        self.conn.change_window_attributes(
            window.window,
            &ChangeWindowAttributesAux::new().event_mask(
                EventMask::KEY_PRESS
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::ENTER_WINDOW
                    | EventMask::PROPERTY_CHANGE,
            ),
        )?;

        self.atoms.change_atom_prop(
            window.window,
            self.atoms.net_wm_allowed_actions,
            &[self.atoms.net_wm_action_fullscreen],
        )?;

        self.atoms.change_cardinal_prop(
            window.window,
            self.atoms.net_frame_extents,
            &[
                self.config.border_size,
                self.config.border_size,
                self.config.border_size,
                self.config.border_size,
            ],
        )?;

        self.conn.change_property32(
            PropMode::REPLACE,
            window.window,
            self.atoms.wm_state,
            self.atoms.wm_state,
            &[1, 0],
        )?;

        self.conn.grab_server()?;
        self.conn.change_save_set(SetMode::INSERT, window.window)?;
        self.conn
            .reparent_window(window.window, window.frame_window, 0, 0)?;
        self.map(window)?;
        self.conn.ungrab_server()?;
        Ok(())
    }

    pub fn destroy_window(&self, window: &WindowState) -> Res {
        log::trace!("destroying window: {}", window.window);
        self.conn.change_save_set(SetMode::DELETE, window.window)?;
        self.conn
            .reparent_window(window.window, self.screen.root, window.x, window.y)?;
        self.conn.destroy_window(window.frame_window)?;

        Ok(())
    }

    pub fn set_focus_window(&self, windows: &[WindowState], window: &WindowState) -> Res {
        log::trace!("setting focus to: {:?}", window.window);
        self.conn
            .set_input_focus(InputFocus::PARENT, window.window, CURRENT_TIME)?;

        //set borders
        windows.iter().try_for_each(|w| {
            if w.group == WindowGroup::Floating {
                return Ok(());
            }
            self.conn.configure_window(
                w.frame_window,
                &ConfigureWindowAux::new().border_width(self.config.border_size),
            )?;
            self.conn.change_window_attributes(
                w.frame_window,
                &ChangeWindowAttributesAux::new().border_pixel(self.colors.main),
            )?;
            Ok::<(), ReplyOrIdError>(())
        })?;

        self.conn.change_window_attributes(
            window.frame_window,
            &ChangeWindowAttributesAux::new().border_pixel(self.colors.secondary),
        )?;

        self.atoms.change_window_prop(
            self.screen.root,
            self.atoms.net_active_window,
            &[window.window],
        )?;

        Ok(())
    }

    pub fn get_focus(&self) -> Result<u32, ReplyOrIdError> {
        Ok(self.conn.get_input_focus()?.reply()?.focus)
    }

    pub fn config_window_from_state(&self, window: &WindowState) -> Res {
        log::trace!("configuring window {} from state", window.window);
        self.conn
            .configure_window(
                window.frame_window,
                &ConfigureWindowAux {
                    x: Some(i32::from(window.x)),
                    y: Some(i32::from(window.y)),
                    width: Some(u32::from(window.width)),
                    height: Some(u32::from(window.height)),
                    border_width: None,
                    sibling: None,
                    stack_mode: None,
                },
            )?
            .check()?;
        self.conn
            .configure_window(
                window.window,
                &ConfigureWindowAux {
                    x: Some(0),
                    y: Some(0),
                    width: Some(u32::from(window.width)),
                    height: Some(u32::from(window.height)),
                    border_width: None,
                    sibling: None,
                    stack_mode: None,
                },
            )?
            .check()?;

        Ok(())
    }

    pub fn set_focus_to_root(&self) -> Result<(), ReplyOrIdError> {
        log::trace!("setting focus to root");
        self.conn
            .set_input_focus(InputFocus::NONE, 1_u32, CURRENT_TIME)?;

        self.atoms
            .change_window_prop(self.screen.root, self.atoms.net_active_window, &[1])?;
        Ok(())
    }

    pub fn kill_focus(&self, focus: Id) -> Res {
        log::trace!("killing focus window {focus}");
        self.conn.send_event(
            false,
            focus,
            EventMask::NO_EVENT,
            ClientMessageEvent::new(
                32,
                focus,
                self.atoms.wm_protocols,
                [self.atoms.wm_delete_window, 0, 0, 0, 0],
            ),
        )?;
        Ok(())
    }

    pub fn set_fullscreen(&self, window: &WindowState) -> Res {
        log::trace!("setting window to fullscreen {}", window.window);
        self.config_window_from_state(window)?;
        self.atoms.change_atom_prop(
            window.window,
            self.atoms.net_wm_state,
            &[self.atoms.net_wm_state_fullscreen],
        )?;
        self.conn.configure_window(
            window.frame_window,
            &ConfigureWindowAux::new().border_width(0),
        )?;
        Ok(())
    }

    pub fn update_client_list(&self, state: &StateHandler) -> Res {
        let ids: Vec<Id> = state.tags[state.active_tag]
            .windows
            .iter()
            .map(|w| w.window)
            .collect();

        self.atoms
            .change_window_prop(self.screen.root, self.atoms.net_client_list, &ids)?;
        Ok(())
    }

    pub fn update_active_desktop(&self, tag: u32) -> Res {
        self.atoms
            .change_window_prop(self.screen.root, self.atoms.net_current_desktop, &[tag])?;
        Ok(())
    }

    pub fn update_window_desktop(&self, window: Window, tag: u32) -> Res {
        self.atoms
            .change_window_prop(window, self.atoms.net_wm_desktop, &[tag])?;
        Ok(())
    }

    pub fn get_window_name(&self, window: Window) -> Result<String, ReplyOrIdError> {
        log::trace!("getting window name of {window}");

        let result = String::from_utf8(
            self.conn
                .get_property(
                    false,
                    window,
                    self.atoms.net_wm_name,
                    self.atoms.utf8_string,
                    0,
                    100,
                )?
                .reply()?
                .value,
        )
        .unwrap_or_default();

        if result.is_empty() {
            let result = String::from_utf8(
                self.conn
                    .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 100)?
                    .reply()?
                    .value,
            )
            .unwrap_or_default();
            Ok(result)
        } else {
            Ok(result)
        }
    }

    pub fn create_gc(&self, gc: Id, color_background: Id, color_foreground: Id) -> Res {
        self.conn.create_gc(
            gc,
            self.screen.root,
            &CreateGCAux::new()
                .graphics_exposures(0)
                .background(color_background)
                .foreground(color_foreground),
        )?;
        Ok(())
    }

    pub fn create_pixmap_from_win(&self, pixmap: Pixmap, window: &WindowState) -> Res {
        self.conn.create_pixmap(
            self.screen.root_depth,
            pixmap,
            window.window,
            window.width,
            window.height,
        )?;
        Ok(())
    }

    pub fn create_window(&self, window: &WindowState) -> Res {
        self.conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window.window,
            self.screen.root,
            0,
            0,
            window.width,
            window.height,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )?;
        Ok(())
    }

    pub fn clear_window(&self, window: &WindowState) -> Res {
        self.conn.clear_area(
            false,
            window.window,
            window.x,
            window.y,
            window.width,
            window.height,
        )?;
        Ok(())
    }

    pub fn copy_window_to_window(
        &self,
        gc: Gcontext,
        window_1: Window,
        window_2: &WindowState,
    ) -> Res {
        self.conn.copy_area(
            window_1,
            window_2.window,
            gc,
            0,
            0,
            0,
            0,
            window_2.width,
            window_2.height,
        )?;
        Ok(())
    }

    pub fn draw_to_pixmap(
        &self,
        pixmap: Pixmap,
        gc: Gcontext,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        data: &[u8],
    ) -> Res {
        if let Err(e) = self
            .conn
            .put_image(
                ImageFormat::Z_PIXMAP,
                pixmap,
                gc,
                width,
                height,
                x,
                y,
                0,
                self.screen.root_depth,
                data,
            )?
            .check()
        {
            log::error!("error putting image! {e}");
        }
        Ok(())
    }

    fn set_cursor(&self) -> Res {
        let cursor = cursor::Handle::new(
            self.conn,
            self.screen_num,
            &resource_manager::new_from_default(self.conn)?,
        )?
        .reply()?
        .load_cursor(self.conn, "left_ptr")?;
        self.conn.change_window_attributes(
            self.screen.root,
            &ChangeWindowAttributesAux::new().cursor(cursor),
        )?;
        Ok(())
    }

    fn add_heartbeat_window(&self) -> Res {
        let proof_window_id = self.conn.generate_id()?;

        self.conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            proof_window_id,
            self.screen.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )?;

        self.atoms.change_window_prop(
            self.screen.root,
            self.atoms.net_supporting_wm_check,
            &[proof_window_id],
        )?;
        self.atoms.change_window_prop(
            proof_window_id,
            self.atoms.net_supporting_wm_check,
            &[proof_window_id],
        )?;
        self.atoms
            .change_string_prop(proof_window_id, self.atoms.net_wm_name, "hematite")?;
        Ok(())
    }

    fn grab_keys(&self, handler: &KeyHandler) -> Res {
        handler.hotkeys.iter().try_for_each(|h| {
            self.conn
                .grab_key(
                    true,
                    self.screen.root,
                    h.modifier,
                    h.code,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )?
                .check()
        })?;
        Ok(())
    }
}

pub fn spawn_command(command: &str) {
    match Command::new("sh").arg("-c").arg(command).spawn() {
        Ok(_) => (),
        Err(e) => log::error!("error when spawning command {e:?}"),
    }
}

fn become_window_manager<C: Connection>(conn: &C, root: u32) -> Res {
    let change = ChangeWindowAttributesAux::default().event_mask(
        EventMask::SUBSTRUCTURE_REDIRECT
            | EventMask::SUBSTRUCTURE_NOTIFY
            | EventMask::KEY_PRESS
            | EventMask::PROPERTY_CHANGE,
    );
    let result = conn.change_window_attributes(root, &change)?.check();

    if let Err(ReplyError::X11Error(ref error)) = result {
        if error.error_kind == ErrorKind::Access {
            log::error!("another wm is running");
            exit(1);
        }
    } else {
        log::info!("became window manager successfully");
    }
    Ok(())
}

fn get_color_id<C: Connection>(
    conn: &C,
    screen: &Screen,
    color: Color,
) -> Result<Id, ReplyOrIdError> {
    Ok(conn
        .alloc_color(screen.default_colormap, color.red, color.green, color.blue)?
        .reply()?
        .pixel)
}
