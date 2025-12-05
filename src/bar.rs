//!
//! This module provides a status bar that displays tag and window information as well as status text provided by the user.
use std::collections::HashMap;

use fontdue::Metrics;
use x11rb::{
    errors::ReplyOrIdError,
    protocol::xproto::{Gcontext, Pixmap, Rectangle, Window},
};

use crate::{
    config::Config,
    connection::{Colors, ConnectionActionExt, ConnectionAtomExt, ConnectionStateExt, Res},
    render::{Image, ImageHandler},
    state::{WindowGroup, WindowState},
};

/// The number of available tags.
const TAG_COUNT: usize = 9;

/// A cache for the left side of the bar to minimize redraws.
pub struct Cache {
    /// Icons pertaining to specific windows.
    pub icons: HashMap<Window, Image>,
    /// Window names pertaining to specific windows.
    ///
    /// Names still have to be asked to see if they are updated, but the draw call can be avoided.
    pub names: HashMap<Window, String>,
    /// The actively used tag.
    active_tag: usize,
    /// The tags which have a window in them, represented as a bitmask.
    used_tags: u16,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            icons: HashMap::default(),
            names: HashMap::default(),
            active_tag: usize::MAX,
            used_tags: Default::default(),
        }
    }
}

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
    image: ImageHandler,
    /// A cache for reducing draw calls.
    pub cache: Cache,
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
        let text = ImageHandler::new(config);

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
            image: text,
            cache: Cache::default(),
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
        active_tag: usize,
        tag_bitmask: u16,
        conn: &(impl ConnectionActionExt + ConnectionStateExt + ConnectionAtomExt),
        active_window: Option<Window>,
    ) -> Res {
        if self.cache.active_tag != active_tag || self.cache.used_tags != tag_bitmask {
            conn.fill_rectangle(
                self.pixmap,
                self.inverted_gc,
                Rectangle {
                    x: 0,
                    y: 0,
                    width: self.bar.height * TAG_COUNT as u16,
                    height: self.bar.height,
                },
            )?;

            self.draw_rectangles(active_tag, tag_bitmask, conn)?;
            self.draw_tag_letters(conn, active_tag, self.base_y)?;
            self.cache.active_tag = active_tag;
            self.cache.used_tags = tag_bitmask;
        }

        if let Some(window) = active_window {
            let text = conn.get_window_name(window)?;
            if let Some(cached_text) = self.cache.names.get(&window) {
                if *cached_text != text {
                    self.draw_window_properties(conn, &text)?;
                    self.cache.names.entry(window).and_modify(|s| *s = text);
                }
            } else {
                self.draw_window_properties(conn, &text)?;
                self.cache.names.entry(window).and_modify(|s| *s = text);
            }
            self.draw_icon(conn, window)?;
        } else {
            self.draw_window_properties(conn, "")?;
        }

        self.draw_status_bar(conn)?;
        self.clear_and_copy_bar(conn)?;
        Ok(())
    }

    /// aaa
    fn draw_window_properties(&mut self, conn: &impl ConnectionActionExt, text: &str) -> Res {
        // let length = self.text.get_text_length(text);
        conn.fill_rectangle(
            self.pixmap,
            self.inverted_gc,
            Rectangle {
                x: self.bar.height as i16 * TAG_COUNT as i16,
                y: 0,
                width: self.bar.width - self.bar.height * TAG_COUNT as u16,
                height: self.bar.height,
            },
        )?;
        self.draw_text(conn, text, self.base_x + 16, self.base_y)?;
        Ok(())
    }

    /// Draws the window icon to the bar.
    ///
    /// An icon is an ARGB byte sequence with the first eight bytes being the width and height of the icon.
    ///
    /// An icon can be of any size and usually we need to scale it up or down to match the font size.
    ///
    /// We also cache icons pertaining to a window to not have to calculate and draw the icon every refresh, and drop them when the window is dropped.
    /// # Errors
    /// Returns an error if the window is invalid.
    fn draw_icon(
        &mut self,
        conn: &(impl ConnectionActionExt + ConnectionAtomExt),
        window: Window,
    ) -> Res {
        let icon = if let Some(icon) = self.cache.icons.get(&window) {
            icon
        } else {
            let icon_with_dimensions = conn.get_icon(window)?;
            if icon_with_dimensions.is_empty() {
                return Ok(());
            }

            let width = u32::from_ne_bytes(
                icon_with_dimensions[0..4]
                    .try_into()
                    .unwrap_or([0, 0, 0, 0]),
            );
            let height = u32::from_ne_bytes(
                icon_with_dimensions[4..8]
                    .try_into()
                    .unwrap_or([0, 0, 0, 0]),
            );

            let Some(icon) = self.image.resize_image_to_text_height(Image {
                width,
                height,
                data: icon_with_dimensions,
            }) else {
                return Ok(());
            };

            self.cache.icons.insert(window, icon);
            let Some(icon) = self.cache.icons.get(&window) else {
                return Ok(());
            };
            icon
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

        let length = self.image.get_text_length(&status_text);

        conn.fill_rectangle(
            self.pixmap,
            self.inverted_gc,
            Rectangle {
                x: self.bar.width as i16 - length,
                y: 0,
                width: length as u16,
                height: self.bar.height,
            },
        )?;

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
    fn draw_rectangles(
        &mut self,
        active_tag: usize,
        tag_bitmask: u16,
        conn: &impl ConnectionActionExt,
    ) -> Res {
        conn.fill_rectangle(
            self.pixmap,
            self.gc,
            self.create_tag_rectangle(active_tag + 1),
        )?;

        if tag_is_used(tag_bitmask, active_tag) {
            conn.fill_rectangle(
                self.pixmap,
                self.inverted_gc,
                Rectangle {
                    x: self.bar.height as i16 * (active_tag as i16) + self.bar.height as i16 / 7,
                    y: self.bar.height as i16 / 7,
                    width: self.bar.height / 6,
                    height: self.bar.height / 6,
                },
            )?;
        }

        (0..TAG_COUNT)
            .filter(|x| *x != active_tag && tag_is_used(tag_bitmask, *x))
            .map(|x| Rectangle {
                x: self.bar.height as i16 * (x as i16) + self.bar.height as i16 / 7,
                y: self.bar.height as i16 / 7,
                width: self.bar.height / 6,
                height: self.bar.height / 6,
            })
            .try_for_each(|r| conn.fill_rectangle(self.pixmap, self.gc, r))?;

        Ok(())
    }

    /// Draws the numbers of the tags onto the bar.
    ///
    /// The active tag's number has inverted colors.
    fn draw_tag_letters(
        &mut self,
        conn: &impl ConnectionActionExt,
        active_tag: usize,
        base_y: i16,
    ) -> Res {
        (1..=TAG_COUNT).try_for_each(|x| {
            if x == active_tag + 1 {
                let (metrics, data) = self.image.rasterize_letter(
                    char::from_digit(x as u32, 10).unwrap_or_default(),
                    self.image.colors.foreground,
                    self.image.colors.background,
                );
                let base_x = self.bar.height * (x as u16 - 1)
                    + (self.bar.height / 2 - (metrics.advance_width as u16 / 2));
                self.put_text_data(conn, metrics, data.as_slice(), base_x as i16, base_y)?;
            } else {
                let (metrics, data) = self.image.rasterize_letter(
                    char::from_digit(x as u32, 10).unwrap_or_default(),
                    self.image.colors.background,
                    self.image.colors.foreground,
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
    fn draw_text(
        &self,
        conn: &impl ConnectionActionExt,
        text: &str,
        base_x: i16,
        base_y: i16,
    ) -> Res {
        let mut total_width = 0;
        text.chars().try_for_each(|c| {
            let (metrics, data) = self.image.rasterize_letter(
                c,
                self.image.colors.background,
                self.image.colors.foreground,
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

/// Returns true if the specified tag has a window in it.
/// 
/// The bitmask represents a list of booleans indicating whether a tag has a window in it.
fn tag_is_used(bitmask: u16, tag: usize) -> bool {
    bitmask & (1 << tag) != 0
}
