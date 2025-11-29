//!
//! This module provides a font helper that rasterizes and paints the specified letters.
use std::{fs, process::exit};

use fontdue::{Font, Metrics};

use crate::config::Config;
/// The font's foreground and background color.
pub struct Colors {
    /// This determines the text's color.
    pub foreground: (u8, u8, u8),
    /// This determines the text's background.
    pub background: (u8, u8, u8),
}

/// A helper for drawing with fonts.
pub struct TextHandler {
    /// The font provided in configuration.
    font: Font,
    /// The metrics (width, height) of the char 'A'.
    pub metrics: Metrics,
    /// The colors of the font to draw with.
    pub colors: Colors,
}

impl TextHandler {
    /// Creates a new helper.
    #[must_use]
    pub fn new(config: &Config) -> Self {
        let font = match get_font_file(&config.font) {
            Ok(f) => f,
            Err(e) => {
                log::error!("couldnt open font! {e}");
                exit(0);
            }
        };

        let metrics = font.metrics('A', config.font_size as f32);

        Self {
            font,
            metrics,
            colors: Colors {
                foreground: (
                    (config.main_color.red / 257) as u8,
                    (config.main_color.green / 257) as u8,
                    (config.main_color.blue / 257) as u8,
                ),
                background: (
                    (config.secondary_color.red / 257) as u8,
                    (config.secondary_color.green / 257) as u8,
                    (config.secondary_color.blue / 257) as u8,
                ),
            },
        }
    }

    /// Creates a BGRA byte array out of a letter.
    #[must_use]
    pub fn rasterize_letter(
        &self,
        c: char,
        foreground: (u8, u8, u8),
        background: (u8, u8, u8),
    ) -> (Metrics, Vec<u8>) {
        let (metrics, bytes) = self.font.rasterize(c, self.metrics.height as f32);
        let mut data: Vec<u8> = vec![0u8; metrics.width * metrics.height * 4];
        bytes.iter().enumerate().for_each(|(i, &a)| {
            let j = i * 4;
            data[j] = alpha_interpolate(foreground.2, background.2, a);
            data[j + 1] = alpha_interpolate(foreground.1, background.1, a);
            data[j + 2] = alpha_interpolate(foreground.0, background.0, a);
            data[j + 3] = 0xFF;
        });
        (metrics, data)
    }

    /// Gets the metrics of the specified letter.
    #[must_use]
    pub fn get_metrics(&self, c: char) -> Metrics {
        self.font.metrics(c, self.metrics.height as f32)
    }
}

/// Determines the blended combination of both colors with the specified alpha mask.
#[must_use]
fn alpha_interpolate(color1: u8, color2: u8, alpha: u8) -> u8 {
    ((u32::from(color1) * u32::from(alpha) + (255 - u32::from(alpha)) * u32::from(color2)) / 255)
        as u8
}

#[must_use] 
pub fn blend_image_with_background(bytes: &[u8], background: (u8, u8, u8)) -> Vec<u8> {
    (0..bytes.len()-3)
        .step_by(4)
        .flat_map(|i| {
            [
                alpha_interpolate(bytes[i], background.2, bytes[i+3]),
                alpha_interpolate(bytes[i+1], background.1, bytes[i+3]),
                alpha_interpolate(bytes[i+2], background.0, bytes[i+3]),
                0xFF,
            ]
        })
        .collect()
}

/// Loads a font based on the specified path.
///
/// May return an error if the file is missing or the font is damaged.
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
