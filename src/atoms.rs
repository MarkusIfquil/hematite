use std::collections::HashMap;

use x11rb::{
    connection::Connection,
    errors::ReplyOrIdError,
    protocol::xproto::{Atom, AtomEnum, ConnectionExt, PropMode, Screen, Window},
    wrapper::ConnectionExt as _,
};

use crate::actions::Res;

pub struct Atoms<'a, C> {
    conn: &'a C,
    pub net_supported: Atom,
    pub net_client_list: Atom,
    pub net_number_of_desktops: Atom,
    pub net_desktop_geometry: Atom,
    pub net_desktop_viewport: Atom,
    pub net_current_desktop: Atom,
    pub net_active_window: Atom,
    pub net_workarea: Atom,
    pub net_supporting_wm_check: Atom,
    pub net_frame_extents: Atom,
    pub net_wm_name: Atom,
    pub net_wm_desktop: Atom,
    pub net_wm_state: Atom,
    pub net_wm_state_fullscreen: Atom,
    pub net_wm_allowed_actions: Atom,
    pub net_wm_action_fullscreen: Atom,
    pub utf8_string: Atom,
    pub wm_protocols: Atom,
    pub wm_state: Atom,
    pub wm_delete_window: Atom,
}

impl<'a, C: Connection> Atoms<'a, C> {
    pub fn new(conn: &'a C, screen: &Screen) -> Result<Self,ReplyOrIdError> {
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

        let new_self = Self {
            conn,
            net_supported: atoms["_NET_SUPPORTED"],
            net_client_list: atoms["_NET_CLIENT_LIST"],
            net_number_of_desktops: atoms["_NET_NUMBER_OF_DESKTOPS"],
            net_desktop_geometry: atoms["_NET_DESKTOP_GEOMETRY"],
            net_desktop_viewport: atoms["_NET_DESKTOP_VIEWPORT"],
            net_current_desktop: atoms["_NET_CURRENT_DESKTOP"],
            net_active_window: atoms["_NET_ACTIVE_WINDOW"],
            net_workarea: atoms["_NET_WORKAREA"],
            net_supporting_wm_check: atoms["_NET_SUPPORTING_WM_CHECK"],
            net_frame_extents: atoms["_NET_FRAME_EXTENTS"],
            net_wm_name: atoms["_NET_WM_NAME"],
            net_wm_desktop: atoms["_NET_WM_DESKTOP"],
            net_wm_state: atoms["_NET_WM_STATE"],
            net_wm_state_fullscreen: atoms["_NET_WM_STATE_FULLSCREEN"],
            net_wm_allowed_actions: atoms["_NET_WM_ALLOWED_ACTIONS"],
            net_wm_action_fullscreen: atoms["_NET_WM_ACTION_FULLSCREEN"],
            utf8_string: atoms["UTF8_STRING"],
            wm_protocols: atoms["WM_PROTOCOLS"],
            wm_state: atoms["WM_STATE"],
            wm_delete_window: atoms["WM_DELETE_WINDOW"],
        };
        new_self.setup_atoms(screen,&atom_nums)?;
        Ok(new_self)
    }
    pub fn get_atom_name(&self, atom: Atom) -> Result<String, ReplyOrIdError> {
        String::from_utf8(self.conn.get_atom_name(atom)?.reply()?.name)
            .map_or_else(|_| Ok(String::new()), Ok)
    }

    pub fn setup_atoms(&self, screen: &Screen, atom_nums: &[Atom]) -> Res {
        self.change_atom_prop(screen.root, self.net_supported, atom_nums)?;
        self.change_cardinal_prop(screen.root, self.net_number_of_desktops, &[9])?;
        self.change_cardinal_prop(
            screen.root,
            self.net_desktop_geometry,
            &[
                u32::from(screen.width_in_pixels),
                u32::from(screen.height_in_pixels),
            ],
        )?;
        self.change_cardinal_prop(screen.root, self.net_desktop_viewport, &[0, 0])?;
        self.change_cardinal_prop(
            screen.root,
            self.net_workarea,
            &[
                0,
                0,
                u32::from(screen.width_in_pixels),
                u32::from(screen.height_in_pixels),
            ],
        )?;
        Ok(())
    }
    pub fn change_atom_prop(&self, window: Window, property: Atom, data: &[u32]) -> Res {
        self.conn
            .change_property32(PropMode::REPLACE, window, property, AtomEnum::ATOM, data)?
            .check()?;
        Ok(())
    }

    pub fn change_window_prop(&self, window: Window, property: Atom, data: &[u32]) -> Res {
        self.conn
            .change_property32(PropMode::REPLACE, window, property, AtomEnum::WINDOW, data)?;
        Ok(())
    }

    pub fn change_cardinal_prop(&self, window: Window, property: Atom, data: &[u32]) -> Res {
        self.conn.change_property32(
            PropMode::REPLACE,
            window,
            property,
            AtomEnum::CARDINAL,
            data,
        )?;
        Ok(())
    }

    pub fn change_string_prop(&self, window: Window, property: Atom, data: &str) -> Res {
        self.conn.change_property8(PropMode::REPLACE, window, property, AtomEnum::STRING, data.as_bytes())?;
        Ok(())
    }

    pub fn remove_atom_prop(&self, window: Window, property: Atom) -> Res {
        self.change_atom_prop(window, property, &[0])?;
        Ok(())
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
