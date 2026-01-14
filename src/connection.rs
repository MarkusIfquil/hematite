//!
//! This module extends `x11rb`'s `Connection` trait to interact with the manager state, provide more complicated actions, and manage atoms.
use std::process::Command;
use std::process::exit;

use x11rb::protocol::render::Color;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xproto::Pixmap;
use x11rb::protocol::xproto::Rectangle;
use x11rb::protocol::xproto::StackMode;
use x11rb::wrapper::ConnectionExt as _;
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
    state::{WindowGroup, WindowState},
};

/// A shorthand for `Result<(),ReplyOrIdError`.
///
/// The `ReplyOrIdError` is the main error that is used when handling the X11 connection, so many functions return this type to be able to use the `?` syntax and bubble the error.
pub type Res = Result<(), ReplyOrIdError>;
/// An integer handle to an X11 resource.
///
/// The resource may be a window, pixmap, colormap, graphics context, etc. It is preferred to use the resource's unique type (e.g. `GContext` for gcs) instead.
pub type Id = u32;
/// Contains the ids of all allocated colors.
///
/// Currently only a main and secondary color is defined.
pub struct Colors {
    /// The main color defines the background color, predominantly used in the status bar.
    pub main: Id,
    /// The secondary color defines the text color used in the status bar and the border color of windows.
    pub secondary: Id,
}

/// Defines all the ways the connection interacts with state. Usually a `WindowState` reference is passed as a shorthand for its coordinates and size.
pub trait ConnectionStateExt {
    /// Maps a window and its frame window to the display based on its state.
    ///
    /// # Errors
    /// Returns an error if the window does not exist.
    fn map(&self, window: &WindowState) -> Res;
    /// Unmaps a window and its frame window from the display.
    /// # Errors
    /// Returns an error if the window does not exist.
    fn unmap(&self, window: &WindowState) -> Res;
    /// Creates a frame window and reparents the window into it. Also adds `EventMask`s to the windows.
    /// # Errors
    /// Returns an error if the window does not exist.
    fn add_window(&self, window: &WindowState) -> Res;
    /// Destroys the frame window of a window and reparents the window to the root window, allowing it to close naturally.
    /// # Errors
    /// Returns an error if the frame window does not exist.
    fn destroy_frame_window(&self, window: &WindowState) -> Res;
    /// Creates a window from its state.
    /// # Errors
    /// Returns an error if the window couldn't be created.
    fn create_window(&self, window: &WindowState) -> Res;
    /// Clears the window's contents.
    /// # Errors
    /// Returns an error if the window does not exist.
    fn clear_window(&self, window: &WindowState) -> Res;
    /// Configures the window's size and position based on its state.
    /// # Errors
    /// Returns an error if the window does not exist or if it goes beyond the bounds of the screen.
    fn config_window_from_state(&self, window: &WindowState) -> Res;
    /// Sets the window's size to be the entire screen and lets it know it's in fullscreen mode.
    ///
    /// Fullscreen windows are in the `Floating` group to avoid having them accidentally tiled.
    /// # Errors
    /// Returns an error if the window does not exist or if the window can't be resized.
    fn set_fullscreen(&self, window: &WindowState) -> Res;
    /// Removes fullscreen properties from the window.
    ///
    /// # Errors
    /// Returns an error if the window does not exist.
    fn remove_fullscreen(&self, window: &WindowState) -> Res;
    /// Creates a pixmap (basically an off screen window to draw to) from its state.
    /// # Errors
    /// Returns an error if the window does not exist.
    fn create_pixmap_from_win(&self, pixmap: Pixmap, window: &WindowState) -> Res;
    /// Sets the currently focused window's border to be visible and gives it the input focus.
    /// # Errors
    /// Returns an error if the window or its frame window does not exist.
    fn set_focus_window(&self, windows: &[WindowState], focus: &WindowState) -> Res;
    /// Copies a window or pixmap's contents into another window.
    ///
    /// Only the second window's state needs to be known in order to fill the entire window. It is assumed that both windows are the same size.
    /// # Errors
    /// Returns an error if the graphics context or the windows do not exist.
    fn copy_window_to_window(&self, gc: Gcontext, window_1: Window, window_2: &WindowState) -> Res;
    /// Configures a window based on a `ConfigureRequestEvent`.
    /// # Errors
    /// Returns an error if the event specifies the wrong parameters.
    fn handle_config(&self, event: ConfigureRequestEvent, window: &mut WindowState) -> Res;
}

/// Defines the more abstract directions you can give to the X11 server, like drawing to a pixmap or killing the focused window.
pub trait ConnectionActionExt {
    /// Gets the window with the input focus.
    ///
    /// Returns 1 if the root window has the input focus.
    /// # Errors
    /// Returns an error if no window focus is assigned.
    fn get_focus(&self) -> Result<u32, ReplyOrIdError>;
    /// Gives the input focus to the root window.
    /// # Errors
    /// Returns an error if the root window does not exist.
    fn set_focus_to_root(&self) -> Res;
    /// Kills the window which has the input focus.
    /// # Errors
    /// Returns an error if no focus window exists.
    fn kill_focus(&self, focus: Id) -> Res;
    /// Gets the UTF-8 name of the window (if it exists).
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn get_window_name(&self, window: Window) -> Result<String, ReplyOrIdError>;
    /// Creates a graphics context with a background and foreground color.
    /// # Errors
    /// Returns an error if the colors dont exist.
    fn create_gc(&self, gc: Id, color_background: Id, color_foreground: Id) -> Res;
    /// Draws to a pixmap (offscreen window).
    ///
    /// The graphics context does not provide any information and is used as a dummy.
    ///
    /// Data is a BGRA byte sequence. The length of the array must be equal to Width*Height*4.
    /// # Errors
    /// Returns an error if the pixmap or graphics context doesn't exist, or the data is malformed.
    fn draw_to_pixmap(
        &self,
        pixmap: Pixmap,
        gc: Gcontext,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        data: &[u8],
    ) -> Res;
    /// Sets the cursor to be the default left pointer.
    ///
    /// Without this the root window would display an X cursor.
    /// # Errors
    /// Returns an error if no default cursor exists.
    fn set_cursor(&self) -> Res;
    /// Generates a unique id that can be used to identify any X11 resource.
    /// # Errors
    /// Returns an error if no ids are available.
    fn generate_id(&self) -> Result<u32, ReplyOrIdError>;
    /// Grabs keys defined in configuration so that the event handler can later detect when they are pressed.
    /// # Errors
    /// Returns an error if the hotkeys are incorrect.
    fn grab_keys(&self, handler: &KeyHandler) -> Res;
    /// Gets the current screen's width and height in pixels.
    fn get_screen_geometry(&self) -> (u16, u16);
    /// Gets the root window's id.
    fn get_root(&self) -> u32;
    /// Adds a "heartbeat" window.
    ///
    /// Heartbeat windows act as a check that an EWMH compliant window manager is running. They do not have to be mapped and only exist to verify EWMH compliance.
    /// # Errors
    /// Returns an error if the heartbeat window couldn't be created.
    fn add_heartbeat_window(&self) -> Res;
    /// Draws a rectangle to a pixmap.
    ///
    /// The specified graphics context determines its color.
    /// # Errors
    /// Returns an error if the pixmap or graphics context doesn't exist, or the rectangle is incorrect.
    fn fill_rectangle(&self, pixmap: Pixmap, gc: Gcontext, rect: Rectangle) -> Res;
}

/// Defines the methods used to change specific atoms and their data.
pub trait ConnectionAtomExt {
    /// Tells the window the actions it's allowed to perform.
    ///
    /// Currently only the fullscreen action is supported.
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn net_add_allowed_actions(&self, window: Window) -> Res;
    /// Tells the window the size of its surrounding border.
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn net_add_frame_extents(&self, window: Window) -> Res;
    /// Tells the window it is active and displayed.
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn wm_activate_window(&self, window: Window) -> Res;
    /// Tells the window that it has the input focus.
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn net_set_active_window(&self, window: Window) -> Res;
    /// Tells the window that it is in fullscreen mode.
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn net_set_state_fullscreen(&self, window: Window) -> Res;
    /// Tells windows what the currently active tag is.
    /// # Errors
    /// Returns an error if properties can't be changed.
    fn net_update_active_desktop(&self, tag: u32) -> Res;
    /// Tells the window what tag it's in.
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn net_update_window_desktop(&self, window: Window, tag: u32) -> Res;
    /// Updates a list of which windows are managed.
    /// # Errors
    /// Returns an error if the windows are incorrect.
    fn net_update_client_list(&self, windows: &[Window]) -> Res;
    /// Gets the icon data of the window.
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn get_icon(&self, window: Window) -> Result<Vec<u8>, ReplyOrIdError>;
    /// Gets the window hints and determines if the specified window wants to be floating or not.
    ///
    /// Floating logic is determined by checking the min and max widths and heights. If they are the same, then the window is floating and receives its requested width and height in the middle of the screen.
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn should_be_floating(&self, window: Window) -> Result<(u16, u16, bool), ReplyOrIdError>;
    /// Sets the window class of the window.
    /// # Errors
    /// Returns an error if the window doesn't exist.
    fn set_class(&self, class:&str, window: Window) -> Res;
}

/// An implementation of the Connection traits, with additional information like config, screen and atom list.
pub struct ConnectionHandler<'a, C: Connection> {
    /// A connection to the X11 server.
    pub conn: &'a C,
    /// The current display.
    pub screen: &'a Screen,
    /// The screen's id.
    screen_num: usize,
    /// A helper to manage atoms.
    pub atoms: Atoms<'a, C>,
    /// A config for additional information.
    config: Config,
    /// All the ids of the managed colors.
    pub colors: Colors,
}

impl<'a, C: Connection> ConnectionHandler<'a, C> {
    /// Creates a new handler.
    ///
    /// Allocates the specified colors, grabs the specified keys, sets the default cursor and adds a heartbeat window.
    /// # Errors
    /// May return an error if the connection is faulty.
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
}

impl<C: Connection> ConnectionStateExt for ConnectionHandler<'_, C> {
    fn map(&self, window: &WindowState) -> Res {
        log::trace!("handling map of {}", window.window);
        self.conn.map_window(window.frame_window)?;
        self.conn.map_window(window.window)?;
        Ok(())
    }

    fn unmap(&self, window: &WindowState) -> Res {
        log::trace!("handling unmap of {}", window.window);
        self.conn.unmap_window(window.window)?;
        self.conn.unmap_window(window.frame_window)?;
        Ok(())
    }

    fn handle_config(&self, event: ConfigureRequestEvent, window: &mut WindowState) -> Res {
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

        if window.group == WindowGroup::Floating {
            window.x = event.x;
            window.y = event.y;
            window.width = event.width;
            window.height = event.height;
        }

        self.config_window_from_state(window)?;

        Ok(())
    }

    fn add_window(&self, window: &WindowState) -> Res {
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

        self.net_add_allowed_actions(window.window)?;
        self.net_add_frame_extents(window.window)?;
        self.wm_activate_window(window.window)?;

        self.conn.grab_server()?;
        self.conn.change_save_set(SetMode::INSERT, window.window)?;
        self.conn
            .reparent_window(window.window, window.frame_window, 0, 0)?;
        self.map(window)?;
        self.conn.ungrab_server()?;
        Ok(())
    }

    fn destroy_frame_window(&self, window: &WindowState) -> Res {
        log::trace!("destroying window: {}", window.window);
        self.conn.change_save_set(SetMode::DELETE, window.window)?;
        self.conn
            .reparent_window(window.window, self.screen.root, window.x, window.y)?;
        self.conn.destroy_window(window.frame_window)?;

        Ok(())
    }

    fn set_focus_window(&self, windows: &[WindowState], window: &WindowState) -> Res {
        log::trace!("setting focus to: {:?}", window.window);
        self.conn
            .set_input_focus(InputFocus::PARENT, window.window, CURRENT_TIME)?;

        //set borders
        windows.iter().try_for_each(|w| {
            if w.group == WindowGroup::Fullscreen {
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

        self.net_set_active_window(window.window)?;

        Ok(())
    }

    fn config_window_from_state(&self, window: &WindowState) -> Res {
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

    fn set_fullscreen(&self, window: &WindowState) -> Res {
        log::trace!("setting window to fullscreen {}", window.window);
        self.net_set_state_fullscreen(window.window)?;
        self.conn.configure_window(
            window.frame_window,
            &ConfigureWindowAux::new()
                .border_width(0)
                .stack_mode(StackMode::ABOVE),
        )?;
        Ok(())
    }

    fn create_pixmap_from_win(&self, pixmap: Pixmap, window: &WindowState) -> Res {
        self.conn.create_pixmap(
            self.screen.root_depth,
            pixmap,
            window.window,
            window.width,
            window.height,
        )?;
        Ok(())
    }

    fn create_window(&self, window: &WindowState) -> Res {
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

    fn clear_window(&self, window: &WindowState) -> Res {
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

    fn copy_window_to_window(&self, gc: Gcontext, window_1: Window, window_2: &WindowState) -> Res {
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

    fn remove_fullscreen(&self, window: &WindowState) -> Res {
        self.atoms
            .remove_atom_prop(window.window, self.atoms.net_wm_state)?;
        self.conn.configure_window(
            window.frame_window,
            &ConfigureWindowAux::new()
                .stack_mode(StackMode::BELOW)
                .border_width(self.config.border_size),
        )?;
        Ok(())
    }

}

impl<C: Connection> ConnectionActionExt for ConnectionHandler<'_, C> {
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
    fn get_focus(&self) -> Result<u32, ReplyOrIdError> {
        Ok(self.conn.get_input_focus()?.reply()?.focus)
    }
    fn draw_to_pixmap(
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

    fn get_window_name(&self, window: Window) -> Result<String, ReplyOrIdError> {
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

    fn create_gc(&self, gc: Id, color_background: Id, color_foreground: Id) -> Res {
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

    fn set_focus_to_root(&self) -> Result<(), ReplyOrIdError> {
        log::trace!("setting focus to root");
        self.conn
            .set_input_focus(InputFocus::NONE, 1_u32, CURRENT_TIME)?;

        self.atoms
            .change_window_prop(self.screen.root, self.atoms.net_active_window, &[1])?;
        Ok(())
    }

    fn kill_focus(&self, focus: Id) -> Res {
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

    fn generate_id(&self) -> Result<u32, ReplyOrIdError> {
        self.conn.generate_id()
    }

    fn get_screen_geometry(&self) -> (u16, u16) {
        (self.screen.width_in_pixels, self.screen.height_in_pixels)
    }

    fn get_root(&self) -> u32 {
        self.screen.root
    }

    fn fill_rectangle(&self, pixmap: Pixmap, gc: Gcontext, rect: Rectangle) -> Res {
        self.conn
            .poly_fill_rectangle(pixmap, gc, &[rect])?
            .check()?;
        Ok(())
    }
}

impl<C: Connection> ConnectionAtomExt for ConnectionHandler<'_, C> {
    fn set_class(&self, class:&str, window: Window) -> Res {
        self.atoms.change_string_prop(window, self.atoms.wm_class, class)?;
        Ok(())
    }

    fn net_update_client_list(&self, windows: &[Window]) -> Res {
        self.atoms
            .change_window_prop(self.screen.root, self.atoms.net_client_list, windows)?;
        Ok(())
    }

    fn net_update_active_desktop(&self, tag: u32) -> Res {
        self.atoms
            .change_window_prop(self.screen.root, self.atoms.net_current_desktop, &[tag])?;
        Ok(())
    }

    fn net_update_window_desktop(&self, window: Window, tag: u32) -> Res {
        self.atoms
            .change_window_prop(window, self.atoms.net_wm_desktop, &[tag])?;
        Ok(())
    }

    fn net_add_allowed_actions(&self, window: Window) -> Res {
        self.atoms.change_atom_prop(
            window,
            self.atoms.net_wm_allowed_actions,
            &[self.atoms.net_wm_action_fullscreen],
        )?;
        Ok(())
    }

    fn net_add_frame_extents(&self, window: Window) -> Res {
        self.atoms.change_cardinal_prop(
            window,
            self.atoms.net_frame_extents,
            &[
                self.config.border_size,
                self.config.border_size,
                self.config.border_size,
                self.config.border_size,
            ],
        )?;
        Ok(())
    }

    fn wm_activate_window(&self, window: Window) -> Res {
        self.conn.change_property32(
            PropMode::REPLACE,
            window,
            self.atoms.wm_state,
            self.atoms.wm_state,
            &[1, 0],
        )?;
        Ok(())
    }

    fn net_set_active_window(&self, window: Window) -> Res {
        self.atoms
            .change_window_prop(self.screen.root, self.atoms.net_active_window, &[window])?;
        Ok(())
    }

    fn net_set_state_fullscreen(&self, window: Window) -> Res {
        self.atoms.change_atom_prop(
            window,
            self.atoms.net_wm_state,
            &[self.atoms.net_wm_state_fullscreen],
        )?;
        Ok(())
    }

    fn get_icon(&self, window: Window) -> Result<Vec<u8>, ReplyOrIdError> {
        self.atoms
            .get_property(window, self.atoms.net_wm_icon, AtomEnum::CARDINAL)
    }

    fn should_be_floating(&self, window: Window) -> Result<(u16, u16, bool), ReplyOrIdError> {
        unsafe {
            let hints_data = self.atoms.get_property(
                window,
                AtomEnum::WM_NORMAL_HINTS.into(),
                AtomEnum::WM_SIZE_HINTS,
            )?;
            let hints = hints_data.align_to::<u32>().1;
            if hints.len() == 0 {
                return Ok((10, 10, false));
            }
            let width = hints[5];
            let height = hints[6];
            if width == hints[7] && height == hints[8] && width != 0 && height != 0 {
                Ok((width as u16, height as u16, true))
            } else {
                Ok((10, 10, false))
            }
        }
    }
}

/// Spawns a shell command with the specified arguments.
///
/// May log an error if there was an issue with spawning a command.
pub fn spawn_command(command: &str) {
    match Command::new("sh").arg("-c").arg(command).spawn() {
        Ok(_) => (),
        Err(e) => log::error!("error when spawning command {e:?}"),
    }
}

/// Sets the event mask of the root window, and exits if another window manager is running.
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

/// Gets a pixel id from the specified RGB color.
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
