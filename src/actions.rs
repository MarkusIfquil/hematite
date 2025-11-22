use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::process::exit;

use fontdue::Font;

use fontdue::Metrics;
use x11rb::protocol::xproto::Atom;
use x11rb::protocol::xproto::ConnectionExt;
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
            ImageFormat, InputFocus, PropMode, Rectangle, Screen, SetMode, Window, WindowClass,
        },
    },
    resource_manager,
};

use crate::{
    config::Config,
    keys::KeyHandler,
    state::{StateHandler, WindowGroup, WindowState},
};

pub type Res = Result<(), ReplyOrIdError>;
pub type Id = u32;

struct Colors {
    main_color: (u8, u8, u8),
    secondary_color: (u8, u8, u8),
}

pub struct ConnectionHandler<'a, C: Connection> {
    pub conn: &'a C,
    pub screen: &'a Screen,
    screen_num: usize,
    pub id_graphics_context: Gcontext,
    id_inverted_graphics_context: Gcontext,
    pub graphics: (u32, u32),
    pub font: Font,
    font_metrics: Metrics,
    pub atoms: HashMap<String, u32>,
    pub config: Config,
    pub bar: WindowState,
    bar_pixmap: u32,
    colors: Colors,
}

impl<'a, C: Connection> ConnectionHandler<'a, C> {
    pub fn new(conn: &'a C, screen_num: usize, config: &Config) -> Result<Self, ReplyOrIdError> {
        let screen = &conn.setup().roots[screen_num];
        become_window_manager(conn, screen.root)?;

        log::trace!("screen num {screen_num} root {}", screen.root);

        let id_graphics_context = conn.generate_id()?;
        let id_inverted_graphics_context = conn.generate_id()?;

        let atom_strings = vec![
            "_NET_SUPPORTED",
            "_NET_CLIENT_LIST",
            "_NET_NUMBER_OF_DESKTOPS",
            "_NET_DESKTOP_GEOMETRY",
            "_NET_DESKTOP_VIEWPORT",
            "_NET_CURRENT_DESKTOP",
            "_NET_DESKTOP_NAMES",
            "_NET_ACTIVE_WINDOW",
            "_NET_WORKAREA",
            "_NET_SUPPORTING_WM_CHECK",
            "_NET_CLOSE_WINDOW",
            "_NET_MOVERESIZE_WINDOW",
            "_NET_WM_MOVERESIZE",
            "_NET_RESTACK_WINDOW",
            "_NET_FRAME_EXTENTS",
            "_NET_WM_NAME",
            "_NET_WM_DESKTOP",
            "_NET_WM_STATE",
            "_NET_WM_STATE_FULLSCREEN",
            "_NET_WM_ALLOWED_ACTIONS",
            "_NET_WM_ACTION_FULLSCREEN",
            "_NET_WM_USER_TIME",
            "UTF8_STRING",
            "WM_NAME",
            "WM_PROTOCOLS",
            "WM_STATE",
            "WM_DELETE_WINDOW",
        ];

        let atom_nums = get_atom_nums(conn, &atom_strings);
        let atoms = get_atom_mapping(&atom_strings, &atom_nums);

        let main_color = get_color_id(conn, screen, config.main_color)?;
        let secondary_color = get_color_id(conn, screen, config.secondary_color)?;

        let font = match get_font_file(&config.font) {
            Ok(f) => f,
            Err(e) => {
                log::error!("couldnt open font! {e}");
                exit(0);
            }
        };

        let metrics = font.metrics('a', config.font_size as f32);
        let pixmap_id = conn.generate_id()?;

        let handler = ConnectionHandler {
            conn,
            screen,
            screen_num,
            id_graphics_context,
            id_inverted_graphics_context,
            graphics: (main_color, secondary_color),
            font,
            font_metrics: metrics,
            atoms,
            config: config.clone(),
            bar: WindowState {
                window: conn.generate_id()?,
                frame_window: conn.generate_id()?,
                x: 0,
                y: 0,
                width: screen.width_in_pixels,
                height: metrics.height as u16 * 3 / 2,
                group: WindowGroup::Floating,
            },
            bar_pixmap: pixmap_id,
            colors: Colors {
                main_color: (
                    (config.main_color.0 / 257) as u8,
                    (config.main_color.1 / 257) as u8,
                    (config.main_color.2 / 257) as u8,
                ),
                secondary_color: (
                    (config.secondary_color.0 / 257) as u8,
                    (config.secondary_color.1 / 257) as u8,
                    (config.secondary_color.2 / 257) as u8,
                ),
            },
        };

        handler.create_bar_window()?;
        conn.create_pixmap(
            screen.root_depth,
            pixmap_id,
            handler.bar.window,
            handler.bar.width,
            handler.bar.height,
        )?;

        handler.create_gcs(main_color, secondary_color)?;

        handler.grab_keys(&KeyHandler::new(conn, config)?)?;
        handler.set_cursor()?;
        handler.setup_atoms(&atom_nums)?;
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

    pub fn refresh(&self, wm_state: &StateHandler) -> Res {
        log::trace!("refreshing");
        self.draw_bar(wm_state, wm_state.tags[wm_state.active_tag].focus)?;
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
                .background_pixel(self.graphics.0)
                .border_pixel(self.graphics.1),
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

        let allowed_actions = ["_NET_WM_ACTION_FULLSCREEN"].map(|a| self.atoms[a]);

        self.change_atom_prop(window.window, "_NET_WM_ALLOWED_ACTIONS", &allowed_actions)?;

        self.change_cardinal_prop(
            window.window,
            "_NET_FRAME_EXTENTS",
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
            self.atoms["WM_STATE"],
            self.atoms["WM_STATE"],
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
                &ChangeWindowAttributesAux::new().border_pixel(self.graphics.0),
            )?;
            Ok::<(), ReplyOrIdError>(())
        })?;

        self.conn.change_window_attributes(
            window.frame_window,
            &ChangeWindowAttributesAux::new().border_pixel(self.graphics.1),
        )?;

        self.change_window_prop(self.screen.root, "_NET_ACTIVE_WINDOW", &[window.window])?;

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

        self.change_window_prop(self.screen.root, "_NET_ACTIVE_WINDOW", &[1])?;
        Ok(())
    }

    pub fn create_bar_window(&self) -> Res {
        log::trace!("creating bar: {}", self.bar.window);
        self.conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            self.bar.window,
            self.screen.root,
            0,
            0,
            self.screen.width_in_pixels,
            self.font_metrics.height as u16 * 3 / 2,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().background_pixel(self.graphics.0),
        )?;
        self.add_window(&self.bar)?;
        Ok(())
    }

    pub fn kill_focus(&self, focus: u32) -> Res {
        log::trace!("killing focus window {focus}");
        self.conn.send_event(
            false,
            focus,
            EventMask::NO_EVENT,
            ClientMessageEvent::new(
                32,
                focus,
                self.atoms["WM_PROTOCOLS"],
                [self.atoms["WM_DELETE_WINDOW"], 0, 0, 0, 0],
            ),
        )?;
        Ok(())
    }

    pub fn draw_bar(&self, state: &StateHandler, active_window: Option<Window>) -> Res {
        let bar_text: String = match active_window {
            Some(w) => self.get_window_name(w)?,
            None => String::new(),
        }
        .chars()
        .take(50)
        .collect();
        log::trace!("drawing bar with text: {bar_text}");

        let base_x = self.bar.height as i16 * 9 + self.bar.height as i16 / 2;
        let base_y = (self.bar.height as i16 / 2) + self.font_metrics.height as i16 / 5 * 2;

        self.conn
            .poly_fill_rectangle(
                self.bar_pixmap,
                self.id_inverted_graphics_context,
                &[Rectangle {
                    x: 0,
                    y: 0,
                    width: self.bar.width,
                    height: self.bar.height,
                }],
            )?
            .check()?;
        self.draw_rectangles(state)?;
        self.draw_tag_letters(state, base_y)?;
        self.draw_text(&bar_text, base_x, base_y)?;
        self.draw_status_bar()?;
        self.clear_and_copy_bar()?;
        Ok(())
    }

    pub fn draw_status_bar(&self) -> Res {
        let status_text = self.get_window_name(self.screen.root)?;
        log::trace!("drawing root windows name on bar with text: {status_text}");
        let length = status_text.chars().fold(0, |acc, c| {
            let metrics = self.font.metrics(c, self.font_metrics.height as f32);
            acc + metrics.advance_width as i16
        });
        self.draw_text(
            &status_text,
            self.bar.width as i16 - length,
            (self.bar.height as i16 / 2) + self.font_metrics.height as i16 / 3,
        )?;
        Ok(())
    }

    pub fn set_fullscreen(&self, window: &WindowState) -> Res {
        log::trace!("setting window to fullscreen {}", window.window);
        self.config_window_from_state(window)?;
        self.change_atom_prop(
            window.window,
            "_NET_WM_STATE",
            &[self.atoms["_NET_WM_STATE_FULLSCREEN"]],
        )?;
        self.conn.configure_window(
            window.frame_window,
            &ConfigureWindowAux::new().border_width(0),
        )?;
        Ok(())
    }

    pub fn get_atom_name(&self, atom: u32) -> Result<String, ReplyOrIdError> {
        String::from_utf8(self.conn.get_atom_name(atom)?.reply()?.name)
            .map_or_else(|_| Ok(String::new()), Ok)
    }

    pub fn remove_atom_prop(&self, window: Window, property: &str) -> Res {
        self.change_atom_prop(window, property, &[0])?;
        Ok(())
    }

    pub fn update_client_list(&self, state: &StateHandler) -> Res {
        let ids: Vec<u32> = state.tags[state.active_tag]
            .windows
            .iter()
            .map(|w| w.window)
            .collect();

        self.change_window_prop(self.screen.root, "_NET_CLIENT_LIST", &ids)?;
        Ok(())
    }

    pub fn update_active_desktop(&self, tag: u32) -> Res {
        self.change_window_prop(self.screen.root, "_NET_CURRENT_DESKTOP", &[tag])?;
        Ok(())
    }

    pub fn update_window_desktop(&self, window: Window, tag: u32) -> Res {
        self.change_window_prop(window, "_NET_WM_DESKTOP", &[tag])?;
        Ok(())
    }

    fn get_window_name(&self, window: Window) -> Result<String, ReplyOrIdError> {
        log::trace!("getting window name of {window}");

        let result = String::from_utf8(
            self.conn
                .get_property(
                    false,
                    window,
                    self.atoms["_NET_WM_NAME"],
                    self.atoms["UTF8_STRING"],
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

    fn create_gcs(&self, main_color: Id, secondary_color: Id) -> Res {
        self.conn.create_gc(
            self.id_graphics_context,
            self.screen.root,
            &CreateGCAux::new()
                .graphics_exposures(0)
                .background(main_color)
                .foreground(secondary_color),
        )?;

        self.conn.create_gc(
            self.id_inverted_graphics_context,
            self.screen.root,
            &CreateGCAux::new()
                .graphics_exposures(0)
                .background(secondary_color)
                .foreground(main_color),
        )?;
        Ok(())
    }

    fn setup_atoms(&self, atom_nums: &[Atom]) -> Res {
        self.add_heartbeat_window()?;

        self.change_atom_prop(self.screen.root, "_NET_SUPPORTED", atom_nums)?;
        self.change_cardinal_prop(self.screen.root, "_NET_NUMBER_OF_DESKTOPS", &[9])?;
        self.change_cardinal_prop(
            self.screen.root,
            "_NET_DESKTOP_GEOMETRY",
            &[
                u32::from(self.screen.width_in_pixels),
                u32::from(self.screen.height_in_pixels),
            ],
        )?;
        self.change_cardinal_prop(self.screen.root, "_NET_DESKTOP_VIEWPORT", &[0, 0])?;
        self.change_cardinal_prop(
            self.screen.root,
            "_NET_WORKAREA",
            &[
                0,
                0,
                u32::from(self.screen.width_in_pixels),
                u32::from(self.screen.height_in_pixels) - u32::from(self.bar.height),
            ],
        )?;
        Ok(())
    }

    fn clear_and_copy_bar(&self) -> Res {
        self.conn
            .clear_area(
                false,
                self.bar.window,
                self.bar.x,
                self.bar.y,
                self.bar.width,
                self.bar.height,
            )?
            .check()?;
        self.conn
            .copy_area(
                self.bar_pixmap,
                self.bar.window,
                self.id_graphics_context,
                0,
                0,
                0,
                0,
                self.bar.width,
                self.bar.height,
            )?
            .check()?;
        Ok(())
    }

    fn draw_rectangles(&self, state: &StateHandler) -> Res {
        //draw indicator that windows are active in tag
        self.conn.poly_fill_rectangle(
            self.bar_pixmap,
            self.id_graphics_context,
            &(1..=9)
                .filter(|x| *x != state.active_tag + 1 && !state.tags[x - 1].windows.is_empty())
                .map(|x| Rectangle {
                    x: self.bar.height as i16 * (x as i16 - 1) + self.bar.height as i16 / 9,
                    y: self.bar.height as i16 / 9,
                    width: self.bar.height / 7,
                    height: self.bar.height / 7,
                })
                .collect::<Vec<Rectangle>>(),
        )?;

        //draw active tag rect
        self.conn.poly_fill_rectangle(
            self.bar_pixmap,
            self.id_graphics_context,
            &[self.create_tag_rectangle(state.active_tag + 1)],
        )?;

        //draw active tag indicator
        if !state.tags[state.active_tag].windows.is_empty() {
            self.conn.poly_fill_rectangle(
                self.bar_pixmap,
                self.id_inverted_graphics_context,
                &[Rectangle {
                    x: self.bar.height as i16 * (state.active_tag as i16)
                        + self.bar.height as i16 / 9,
                    y: self.bar.height as i16 / 9,
                    width: self.bar.height / 7,
                    height: self.bar.height / 7,
                }],
            )?;
        }
        Ok(())
    }

    fn draw_tag_letters(&self, state: &StateHandler, base_y: i16) -> Res {
        (1..=9).try_for_each(|x| {
            let base_x = self.bar.height * (x as u16 - 1)
                + (self.bar.height / 2 - (self.font_metrics.width as u16 / 2));
            if x == state.active_tag + 1 {
                let (metrics, data) = self.rasterize_letter(
                    char::from_digit(x as u32, 10).unwrap_or_default(),
                    self.colors.main_color,
                    self.colors.secondary_color,
                );
                self.draw_letter(metrics, data.as_slice(), base_x as i16, base_y)?;
            } else {
                let (metrics, data) = self.rasterize_letter(
                    char::from_digit(x as u32, 10).unwrap_or_default(),
                    self.colors.secondary_color,
                    self.colors.main_color,
                );
                self.draw_letter(metrics, data.as_slice(), base_x as i16, base_y)?;
            }
            Ok::<(), ReplyOrIdError>(())
        })?;
        Ok(())
    }

    fn draw_text(&self, text: &str, base_x: i16, base_y: i16) -> Res {
        let mut total_width = 0;
        text.chars().try_for_each(|c| {
            let (metrics, data) =
                self.rasterize_letter(c, self.colors.secondary_color, self.colors.main_color);
            self.draw_letter(metrics, data.as_slice(), base_x + total_width, base_y)?;
            total_width += metrics.advance_width as i16;
            Ok::<(), ReplyOrIdError>(())
        })?;
        Ok(())
    }

    fn rasterize_letter(
        &self,
        c: char,
        color1: (u8, u8, u8),
        color2: (u8, u8, u8),
    ) -> (Metrics, Vec<u8>) {
        let (metrics, bytes) = self.font.rasterize(c, self.font_metrics.height as f32);
        let mut data: Vec<u8> = vec![0u8; metrics.width * metrics.height * 4];
        bytes.iter().enumerate().for_each(|(i, &a)| {
            let j = i * 4;
            data[j] = alpha_interpolate(color1.2, color2.2, a);
            data[j + 1] = alpha_interpolate(color1.1, color2.1, a);
            data[j + 2] = alpha_interpolate(color1.0, color2.0, a);
            data[j + 3] = 0xFF;
        });
        (metrics, data)
    }

    fn draw_letter(&self, metrics: Metrics, data: &[u8], base_x: i16, base_y: i16) -> Res {
        if let Err(e) = self
            .conn
            .put_image(
                ImageFormat::Z_PIXMAP,
                self.bar_pixmap,
                self.id_graphics_context,
                metrics.width as u16,
                metrics.height as u16,
                base_x + metrics.xmin as i16,
                base_y - metrics.height as i16 - metrics.ymin as i16,
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

    fn create_tag_rectangle(&self, x: usize) -> Rectangle {
        Rectangle {
            x: self.bar.height as i16 * (x as i16 - 1),
            y: 0,
            width: self.bar.height,
            height: self.bar.height,
        }
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

    fn change_atom_prop(&self, window: Window, property: &str, data: &[u32]) -> Res {
        self.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms[property],
                AtomEnum::ATOM,
                data,
            )?
            .check()?;
        Ok(())
    }

    fn change_window_prop(&self, window: Window, property: &str, data: &[u32]) -> Res {
        self.conn.change_property32(
            PropMode::REPLACE,
            window,
            self.atoms[property],
            AtomEnum::WINDOW,
            data,
        )?;
        Ok(())
    }

    fn change_cardinal_prop(&self, window: Window, property: &str, data: &[u32]) -> Res {
        self.conn.change_property32(
            PropMode::REPLACE,
            window,
            self.atoms[property],
            AtomEnum::CARDINAL,
            data,
        )?;
        Ok(())
    }

    fn add_heartbeat_window(&self) -> Res {
        let support_atom = "_NET_SUPPORTING_WM_CHECK";
        let name_atom = "_NET_WM_NAME";
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

        self.change_window_prop(self.screen.root, support_atom, &[proof_window_id])?;
        self.change_window_prop(proof_window_id, support_atom, &[proof_window_id])?;
        self.conn.change_property8(
            PropMode::REPLACE,
            proof_window_id,
            self.atoms[name_atom],
            AtomEnum::STRING,
            b"hematite",
        )?;
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

fn get_atom_mapping(atom_strings: &[&str], atom_nums: &[u32]) -> HashMap<String, u32> {
    let mut atoms: HashMap<String, u32> = HashMap::new();
    atom_strings
        .iter()
        .map(std::string::ToString::to_string)
        .zip(atom_nums)
        .for_each(|(k, v)| {
            atoms.insert(k, *v);
        });
    atoms
}

fn get_atom_nums<C: Connection>(conn: &C, atom_strings: &[&str]) -> std::vec::Vec<u32> {
    atom_strings
        .iter()
        .flat_map(|s| -> Result<u32, ReplyOrIdError> {
            Ok(conn.intern_atom(false, s.as_bytes())?.reply()?.atom)
        })
        .collect()
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
    color: (u16, u16, u16),
) -> Result<u32, ReplyOrIdError> {
    Ok(conn
        .alloc_color(screen.default_colormap, color.0, color.1, color.2)?
        .reply()?
        .pixel)
}

fn get_font_file(path: &str) -> Result<Font, Box<dyn std::error::Error>> {
    log::debug!("loading font from {path}");
    let file = match fs::read(path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("couldnt open file! {e}");
            return Err(Box::new(e));
        }
    };

    let font = match Font::from_bytes(file, fontdue::FontSettings::default()) {
        Ok(f) => f,
        Err(e) => {
            log::error!("couldn't make font! {e}");
            return Err(e.into());
        }
    };

    Ok(font)
}

fn alpha_interpolate(color1: u8, color2: u8, alpha: u8) -> u8 {
    ((u32::from(color1) * u32::from(alpha) + (255 - u32::from(alpha)) * u32::from(color2)) / 255)
        as u8
}
