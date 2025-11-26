// Xephyr -br -ac -noreset -screen 800x600 :1
#![warn(clippy::correctness)]
#![warn(clippy::suspicious)]
#![warn(clippy::complexity)]
#![warn(clippy::perf)]
#![warn(clippy::style)]
// #![warn(clippy::nursery)]
#![warn(clippy::pedantic)]
// #![warn(clippy::restriction)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::too_many_lines)]

mod atoms;
mod bar;
mod config;
mod connection;
mod events;
mod keys;
mod state;
mod text;
use crate::{
    bar::BarPainter,
    config::{Config, ConfigDeserialized},
    connection::ConnectionHandler,
    events::EventHandler,
    keys::KeyHandler,
    state::{StateHandler, TilingInfo},
};
use std::{sync::mpsc, thread, time::Duration};
use x11rb::{connection::Connection, errors::ReplyOrIdError};

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        width: conn_handler.screen.width_in_pixels,
        height: conn_handler.screen.height_in_pixels,
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
            if let Err(e) = tx.send(1) {
                log::error!("channel error: {e}");
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }
        Ok(())
    });

    loop {
        if rx.try_recv().is_ok() {
            if let Err(e) = bar.draw_bar(
                &event_handler.state,
                &event_handler.conn,
                event_handler.state.get_focus(),
            ) {
                log::error!("{e}");
            }
        }
        conn.flush()?;
        let event = conn.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            if let Err(e) = event_handler.handle_event(&event) {
                log::error!("{e}");
            }
            event_as_option = conn.poll_for_event().unwrap_or_default();
        }
    }
}
