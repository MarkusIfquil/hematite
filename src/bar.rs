use fontdue::Metrics;
use x11rb::{
    connection::Connection,
    errors::ReplyOrIdError,
    protocol::xproto::{ConnectionExt, Gcontext, Pixmap, Rectangle, Window},
};

use crate::{
    actions::{ConnectionHandler, Res},
    config::Config,
    state::{StateHandler, WindowGroup, WindowState},
    text::TextHandler,
};

pub struct BarPainter {
    pub bar: WindowState,
    base_x: i16,
    base_y: i16,
    pixmap: Pixmap,
    gc: Gcontext,
    inverted_gc: Gcontext,
    text: TextHandler,
}

impl BarPainter {
    pub fn new<C: Connection>(
        conn: &ConnectionHandler<C>,
        config: &Config,
    ) -> Result<Self, ReplyOrIdError> {
        let gc = conn.conn.generate_id()?;
        let inverted_gc = conn.conn.generate_id()?;

        conn.create_gc(gc, conn.colors.main, conn.colors.secondary)?;
        conn.create_gc(inverted_gc, conn.colors.secondary, conn.colors.main)?;
        let text = TextHandler::new(config);

        let pixmap = conn.conn.generate_id()?;

        let bar = WindowState {
            window: conn.conn.generate_id()?,
            frame_window: conn.conn.generate_id()?,
            x: 0,
            y: 0,
            width: conn.screen.width_in_pixels,
            height: text.metrics.height as u16 * 3 / 2,
            group: WindowGroup::Floating,
        };

        let base_x = bar.height as i16 * 9 + bar.height as i16 / 2;
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
        })
    }

    pub fn draw_bar<C: Connection>(
        &self,
        state: &StateHandler,
        conn: &ConnectionHandler<C>,
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

        conn.conn
            .poly_fill_rectangle(
                self.pixmap,
                self.inverted_gc,
                &[Rectangle {
                    x: 0,
                    y: 0,
                    width: self.bar.width,
                    height: self.bar.height,
                }],
            )?
            .check()?;
        self.draw_rectangles(state, conn.conn)?;
        self.draw_tag_letters(conn, state.active_tag, self.base_y)?;
        self.draw_text(conn, &bar_text, self.base_x, self.base_y)?;
        self.draw_status_bar(conn)?;
        self.clear_and_copy_bar(conn)?;
        Ok(())
    }

    pub fn draw_status_bar<C: Connection>(&self, conn: &ConnectionHandler<C>) -> Res {
        let status_text = conn.get_window_name(conn.screen.root)?;

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

    fn clear_and_copy_bar<C: Connection>(&self, conn: &ConnectionHandler<C>) -> Res {
        conn.clear_window(&self.bar)?;
        conn.copy_window_to_window(self.gc, self.pixmap, &self.bar)?;
        Ok(())
    }

    fn draw_rectangles<C: Connection>(&self, state: &StateHandler, conn: &C) -> Res {
        let rectangles = (1..=9)
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

        conn.poly_fill_rectangle(self.pixmap, self.gc, &rectangles)?;

        if !state.tags[state.active_tag].windows.is_empty() {
            conn.poly_fill_rectangle(
                self.pixmap,
                self.inverted_gc,
                &[Rectangle {
                    x: self.bar.height as i16 * (state.active_tag as i16)
                        + self.bar.height as i16 / 7,
                    y: self.bar.height as i16 / 7,
                    width: self.bar.height / 6,
                    height: self.bar.height / 6,
                }],
            )?;
        }

        Ok(())
    }

    fn draw_tag_letters<C: Connection>(
        &self,
        conn: &ConnectionHandler<C>,
        active_tag: usize,
        base_y: i16,
    ) -> Res {
        (1..=9).try_for_each(|x| {
            
            if x == active_tag + 1 {
                let (metrics, data) = self.text.rasterize_letter(
                    char::from_digit(x as u32, 10).unwrap_or_default(), 
                    self.text.colors.main_color,
                    self.text.colors.secondary_color,
                );
                let base_x = self.bar.height * (x as u16 - 1)
                + (self.bar.height / 2 - (metrics.advance_width as u16 / 2));
                self.put_data(conn, metrics, data.as_slice(), base_x as i16, base_y)?;
            } else {
                let (metrics, data) = self.text.rasterize_letter(
                    char::from_digit(x as u32, 10).unwrap_or_default(),
                    self.text.colors.secondary_color,
                    self.text.colors.main_color,
                );
                let base_x = self.bar.height * (x as u16 - 1)
                + (self.bar.height / 2 - (metrics.advance_width as u16 / 2));
                self.put_data(conn, metrics, data.as_slice(), base_x as i16, base_y)?;
            }
            Ok::<(), ReplyOrIdError>(())
        })?;
        Ok(())
    }

    fn draw_text<C: Connection>(
        &self,
        conn: &ConnectionHandler<C>,
        text: &str,
        base_x: i16,
        base_y: i16,
    ) -> Res {
        let mut total_width = 0;
        text.chars().try_for_each(|c| {
            let (metrics, data) = self.text.rasterize_letter(
                c,
                self.text.colors.secondary_color,
                self.text.colors.main_color,
            );
            self.put_data(conn, metrics, data.as_slice(), base_x + total_width, base_y)?;
            total_width += metrics.advance_width as i16;
            Ok::<(), ReplyOrIdError>(())
        })?;
        Ok(())
    }

    const fn create_tag_rectangle(&self, x: usize) -> Rectangle {
        Rectangle {
            x: self.bar.height as i16 * (x as i16 - 1),
            y: 0,
            width: self.bar.height,
            height: self.bar.height,
        }
    }

    pub fn put_data<C: Connection>(
        &self,
        conn: &ConnectionHandler<C>,
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
