//!
//! This module provides a status bar that displays tag and window information as well as status text provided by the user.
use std::collections::HashMap;

use fontdue::Metrics;
use image::{ImageBuffer, Rgba, imageops};
use x11rb::{
    errors::ReplyOrIdError,
    protocol::xproto::{Gcontext, Pixmap, Rectangle, Window},
};

use crate::{
    config::Config,
    connection::{Colors, ConnectionActionExt, ConnectionAtomExt, ConnectionStateExt, Res},
    state::{StateHandler, WindowGroup, WindowState},
    render::TextHandler,
};

/// A window icon.
pub struct Icon {
    /// The width of the icon.
    width: u32,
    /// The height of the icon.
    height: u32,
    /// The bytes (in ARGB) of the image.
    data: Vec<u8>,
}

/// The number of available tags.
const TAG_COUNT: usize = 9;

/// A helper for drawing the bar.
pub struct BarPainter {
    /// The bar as a window with state.
    pub bar: WindowState,
    /// The base x coordinate to draw letters from.
    base_x: i16,
    /// The base y coordinate to draw letters from.
    base_y: i16,
    /// The pixmap associated with the bar.
    pixmap: Pixmap,
    /// The graphics context used to draw to the bar and pixmap.
    gc: Gcontext,
    /// A graphics context with inverted colors to draw highlighted elements.
    inverted_gc: Gcontext,
    /// A helper for drawing text.
    text: TextHandler,
    /// A cache for icons.
    pub icons: HashMap<Window, Icon>,
}

impl BarPainter {
    /// Creates a new helper.
    /// # Errors
    /// Returns an error if the config or colors are incorrect.
    pub fn new(
        conn: &(impl ConnectionActionExt + ConnectionStateExt),
        colors: &Colors,
        config: &Config,
    ) -> Result<Self, ReplyOrIdError> {
        let gc = conn.generate_id()?;
        let inverted_gc = conn.generate_id()?;

        conn.create_gc(gc, colors.main, colors.secondary)?;
        conn.create_gc(inverted_gc, colors.secondary, colors.main)?;
        let text = TextHandler::new(config);

        let pixmap = conn.generate_id()?;

        let bar = WindowState {
            window: conn.generate_id()?,
            frame_window: conn.generate_id()?,
            x: 0,
            y: 0,
            width: conn.get_screen_geometry().0,
            height: text.metrics.height as u16 * 3 / 2,
            group: WindowGroup::Floating,
        };

        let base_x = bar.height as i16 * TAG_COUNT as i16 + bar.height as i16 / 2;
        let base_y = (bar.height as i16 / 2) + text.metrics.height as i16 / 5 * 2;

        conn.create_window(&bar)?;
        conn.add_window(&bar)?;
        conn.create_pixmap_from_win(pixmap, &bar)?;

        Ok(Self {
            bar,
            base_x,
            base_y,
            pixmap,
            gc,
            inverted_gc,
            text,
            icons: HashMap::default(),
        })
    }

    /// Draws the entire bar in this order:
    /// - Clears the pixmap
    /// - Draws tag rectangles
    /// - Draws the tag numbers
    /// - Draws the window icon (if it exists)
    /// - Draws the window text
    /// - Draws the status text
    /// - Copies the pixmap to the bar
    /// # Errors
    /// Returns an error if the connection is faulty or the specified active window does not exist.
    pub fn draw_bar(
        &mut self,
        state: &StateHandler,
        conn: &(impl ConnectionActionExt + ConnectionStateExt + ConnectionAtomExt),
        active_window: Option<Window>,
    ) -> Res {
        let bar_text: String = match active_window {
            Some(w) => conn.get_window_name(w)?,
            None => String::new(),
        }
        .chars()
        .take(50)
        .collect();
        log::trace!("drawing bar with text: {bar_text}");

        conn.fill_rectangle(
            self.pixmap,
            self.inverted_gc,
            Rectangle {
                x: 0,
                y: 0,
                width: self.bar.width,
                height: self.bar.height,
            },
        )?;
        self.draw_rectangles(state, conn)?;
        self.draw_tag_letters(conn, state.active_tag, self.base_y)?;
        if let Some(window) = active_window {
            self.draw_icon(conn, window)?;
        }
        self.draw_text(conn, &bar_text, self.base_x + 16, self.base_y)?;
        self.draw_status_bar(conn)?;
        self.clear_and_copy_bar(conn)?;
        Ok(())
    }

    /// Draws the window icon to the bar.
    fn draw_icon(
        &mut self,
        conn: &(impl ConnectionActionExt + ConnectionAtomExt),
        window: Window,
    ) -> Res {
        let icon = if let Some(icon) = self.icons.get(&window) {
            icon
        } else {
            let icon_with_dimensions = conn.get_icon(window)?;
            if icon_with_dimensions.is_empty() {
                return Ok(());
            }
            let width = u32::from_ne_bytes(icon_with_dimensions[0..4].try_into().unwrap());
            let height = u32::from_ne_bytes(icon_with_dimensions[4..8].try_into().unwrap());
            let ratio = height as f32 / self.text.metrics.height as f32;

            let buff =
                ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, icon_with_dimensions).unwrap();

            let width = (width as f32 / ratio).round() as u32;
            let height = (height as f32 / ratio).round() as u32;

            let icon = Icon {
                width,
                height,
                data: crate::render::blend_image_with_background(
                    &imageops::resize(&buff, width, height, imageops::FilterType::Lanczos3),
                    self.text.colors.foreground,
                ),
            };

            self.icons.insert(window, icon);
            self.icons.get(&window).unwrap()
        };

        conn.draw_to_pixmap(
            self.pixmap,
            self.gc,
            self.base_x - icon.width as i16 / 2,
            self.bar.height as i16 / 2 - icon.height as i16 / 2,
            icon.width as u16,
            icon.height as u16,
            &icon.data,
        )?;
        Ok(())
    }

    /// Draws the status text to the bar.
    ///
    /// The text is drawn on the right side of the bar.
    /// # Errors
    /// Returns an error if the status text overflows.
    fn draw_status_bar(&self, conn: &impl ConnectionActionExt) -> Res {
        let status_text = conn.get_window_name(conn.get_root())?;

        log::trace!("drawing root windows name on bar with text: {status_text}");

        let length = status_text.chars().fold(0, |acc, c| {
            let metrics = self.text.get_metrics(c);
            acc + metrics.advance_width as i16
        });

        self.draw_text(
            conn,
            &status_text,
            self.bar.width as i16 - length,
            self.base_y,
        )?;
        Ok(())
    }

    /// Clears the bar window of its contents and copies the pixmap's contents to it.
    fn clear_and_copy_bar(&self, conn: &impl ConnectionStateExt) -> Res {
        conn.clear_window(&self.bar)?;
        conn.copy_window_to_window(self.gc, self.pixmap, &self.bar)?;
        Ok(())
    }

    /// Draws the rectangles indicating whether a tag has windows in it or not, and the active tag's rectangle
    ///
    /// Indicator rectangles are smaller and occupy the top left side of the outer rectangle.
    ///
    /// These rectangles are drawn on the left side of the bar.
    fn draw_rectangles(&self, state: &StateHandler, conn: &impl ConnectionActionExt) -> Res {
        let rectangles = (1..=TAG_COUNT)
            .filter(|x| *x != state.active_tag + 1 && !state.tags[x - 1].windows.is_empty())
            .map(|x| Rectangle {
                x: self.bar.height as i16 * (x as i16 - 1) + self.bar.height as i16 / 7,
                y: self.bar.height as i16 / 7,
                width: self.bar.height / 6,
                height: self.bar.height / 6,
            })
            .chain(std::iter::once(
                self.create_tag_rectangle(state.active_tag + 1),
            ))
            .collect::<Vec<Rectangle>>();

        rectangles
            .iter()
            .try_for_each(|r| conn.fill_rectangle(self.pixmap, self.gc, *r))?;

        if !state.tags[state.active_tag].windows.is_empty() {
            conn.fill_rectangle(
                self.pixmap,
                self.inverted_gc,
                Rectangle {
                    x: self.bar.height as i16 * (state.active_tag as i16)
                        + self.bar.height as i16 / 7,
                    y: self.bar.height as i16 / 7,
                    width: self.bar.height / 6,
                    height: self.bar.height / 6,
                },
            )?;
        }

        Ok(())
    }

    /// Draws the numbers of the tags onto the bar.
    ///
    /// The active tag's number has inverted colors.
    fn draw_tag_letters(
        &self,
        conn: &impl ConnectionActionExt,
        active_tag: usize,
        base_y: i16,
    ) -> Res {
        (1..=TAG_COUNT).try_for_each(|x| {
            if x == active_tag + 1 {
                let (metrics, data) = self.text.rasterize_letter(
                    char::from_digit(x as u32, 10).unwrap_or_default(),
                    self.text.colors.foreground,
                    self.text.colors.background,
                );
                let base_x = self.bar.height * (x as u16 - 1)
                    + (self.bar.height / 2 - (metrics.advance_width as u16 / 2));
                self.put_text_data(conn, metrics, data.as_slice(), base_x as i16, base_y)?;
            } else {
                let (metrics, data) = self.text.rasterize_letter(
                    char::from_digit(x as u32, 10).unwrap_or_default(),
                    self.text.colors.background,
                    self.text.colors.foreground,
                );
                let base_x = self.bar.height * (x as u16 - 1)
                    + (self.bar.height / 2 - (metrics.advance_width as u16 / 2));
                self.put_text_data(conn, metrics, data.as_slice(), base_x as i16, base_y)?;
            }
            Ok::<(), ReplyOrIdError>(())
        })?;
        Ok(())
    }

    /// Draws the window's name next to the tags.
    ///
    /// If on the root window or the window doesn't have a name, nothing is displayed.
    ///
    /// There is a limit of 50 characters.
    fn draw_text(
        &self,
        conn: &impl ConnectionActionExt,
        text: &str,
        base_x: i16,
        base_y: i16,
    ) -> Res {
        let mut total_width = 0;
        text.chars().try_for_each(|c| {
            let (metrics, data) = self.text.rasterize_letter(
                c,
                self.text.colors.background,
                self.text.colors.foreground,
            );
            self.put_text_data(conn, metrics, data.as_slice(), base_x + total_width, base_y)?;
            total_width += metrics.advance_width as i16;
            Ok::<(), ReplyOrIdError>(())
        })?;
        Ok(())
    }

    /// Creates a rectangle representing a tag on the bar.
    const fn create_tag_rectangle(&self, x: usize) -> Rectangle {
        Rectangle {
            x: self.bar.height as i16 * (x as i16 - 1),
            y: 0,
            width: self.bar.height,
            height: self.bar.height,
        }
    }

    /// Draws the specified byte array to the pixmap at the given coordinates.
    /// # Errors
    /// Returns an error if the metrics or data is faulty.
    fn put_text_data(
        &self,
        conn: &impl ConnectionActionExt,
        metrics: Metrics,
        data: &[u8],
        base_x: i16,
        base_y: i16,
    ) -> Res {
        conn.draw_to_pixmap(
            self.pixmap,
            self.gc,
            base_x + metrics.xmin as i16,
            base_y - metrics.height as i16 - metrics.ymin as i16,
            metrics.width as u16,
            metrics.height as u16,
            data,
        )?;
        Ok(())
    }
}
