//! A small, fast, opinionated X11 tiling window manager.
//!
//! Hematite provides the essential parts of a modern window manager while maintaining a light, easily configurable codebase.
//!
//! The code is organized in separate files, each being their own module:
//! - `connection`: Traits and a struct implementing those traits, wrapping `x11rb`'s Connection trait, providing extra features
//! - `state`: Struct holding the state of windows and desktops
//! - `events`: Parsing events and handling them
//! - `config`: User configuration and hotkey definitions
//! - `bar`: Status bar rendering
//!
//! The flow of the program is:
//! setup -> main event loop -> event catching -> event handling -> back to main loop.
//! 
//! See the `manager` module for the core logic implementation. Everything else is some kind of helper that abstracts away the various properties of the program.

// Xephyr -br -ac -noreset -screen 800x600 :1
#![warn(clippy::correctness)]
#![warn(clippy::suspicious)]
#![warn(clippy::complexity)]
#![warn(clippy::perf)]
#![warn(clippy::style)]
// #![warn(clippy::nursery)]
#![warn(clippy::pedantic)]
#![warn(clippy::missing_docs_in_private_items)]
#![allow(clippy::cast_sign_loss,reason="")]
#![allow(clippy::cast_possible_truncation,reason="")]
#![allow(clippy::cast_possible_wrap,reason="")]
#![allow(clippy::cast_precision_loss,reason="")]
#![allow(clippy::collapsible_if,reason="clippy is weird")]
#![allow(clippy::too_many_arguments,reason="function would have too much indirection")]
#![allow(clippy::too_many_lines,reason="function is generating a config file")]
#![allow(clippy::question_mark_used,reason="no additional error handling required")]
#![allow(clippy::implicit_return,reason="")]
#![allow(clippy::separated_literal_suffix,reason="")]
/// Atom handling.
pub mod atoms;
/// Status bar display.
pub mod bar;
/// Config file parsing.
pub mod config;
/// Connection to the X11 server.
pub mod connection;
/// Event handling and core logic.
pub mod manager;
/// Keypress handling.
pub mod keys;
/// State management of windows and desktops.
pub mod state;
/// Font rendering.
pub mod text;
use crate::{
    bar::BarPainter,
    config::{Config, ConfigDeserialized},
    connection::ConnectionHandler,
    manager::EventHandler,
    keys::KeyHandler,
    state::{StateHandler, TilingInfo},
};
use std::{sync::mpsc, thread};
use core::time::Duration;
use x11rb::{connection::Connection as _, errors::ReplyOrIdError};
use core::error::Error;

/// This function handles various handle initializations and starts the main event loop.
/// 
/// A new thread is spawned to send a tick every second to update the status bar. This helps update the window name text and the status text, which may update frequently.
///
/// # Errors
/// May return and exit if a connection to the X11 can't be made or the connection is dropped.
/// 
/// Event handling errors are simply logged.
pub fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .init();

    let (conn, screen_num) = x11rb::connect(None)?;
    let config = Config::from(ConfigDeserialized::new());
    let conn_handler = ConnectionHandler::new(&conn, screen_num, &config)?;
    let bar = BarPainter::new(&conn_handler, &conn_handler.colors, &config)?;
    let keys = KeyHandler::new(&conn, &config)?;
    let state = StateHandler::new(TilingInfo {
        gap: config.spacing as u16,
        ratio: config.ratio,
        max_width: conn_handler.screen.width_in_pixels,
        max_height: conn_handler.screen.height_in_pixels,
        bar_height: bar.bar.height,
    });

    bar.draw_bar(&state, &conn_handler, None)?;

    let mut event_handler = EventHandler {
        conn: conn_handler,
        state,
        key: keys,
        bar: &bar,
    };

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || -> Result<(), ReplyOrIdError> {
        loop {
            if let Err(error) = tx.send(1_i32) {
                log::error!("channel error: {error}");
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }
        Ok(())
    });

    loop {
        if rx.try_recv().is_ok() {
            if let Err(error) = bar.draw_bar(
                &event_handler.state,
                &event_handler.conn,
                event_handler.state.get_focus(),
            ) {
                log::error!("{error}");
            }
        }
        conn.flush()?;
        let mut potential_event = Some(conn.wait_for_event()?);

        while let Some(event) = potential_event {
            if let Err(error) = event_handler.handle_event(&event) {
                log::error!("{error}");
            }
            potential_event = conn.poll_for_event().unwrap_or_default();
        }
    }
}
