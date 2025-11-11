//! Text rendering using cosmic-text

use crate::display::types::Color;
use cosmic_text::{fontdb, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};
use tiny_skia::Pixmap;

/// Text renderer using cosmic-text for advanced text handling
pub struct TextRenderer {
    font_system: FontSystem,
}

impl TextRenderer {
    /// Create a new text renderer with system fonts
    pub fn new() -> Self {
        let mut font_db = fontdb::Database::new();
        font_db.load_system_fonts();

        let font_system = FontSystem::new_with_locale_and_db(
            sys_locale::get_locale().unwrap_or_else(|| String::from("en-US")),
            font_db,
        );

        Self { font_system }
    }

    /// Draw text on a pixmap at the specified position
    pub fn draw_text(
        &mut self,
        pixmap: &mut Pixmap,
        x: u32,
        y: u32,
        text: &str,
        font_size: u32,
        color: Color,
    ) {
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(font_size as f32, font_size as f32),
        );

        buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );

        let mut swash_cache = SwashCache::new();

        // Render text to pixmap
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical_glyph = glyph.physical((x as f32, y as f32), 1.0);

                if let Some(image) =
                    swash_cache.get_image(&mut self.font_system, physical_glyph.cache_key)
                {
                    // Blend glyph onto pixmap
                    let glyph_x = physical_glyph.x;
                    let glyph_y = physical_glyph.y;

                    for (img_y, row) in image
                        .data
                        .chunks_exact(image.placement.width as usize)
                        .enumerate()
                    {
                        for (img_x, &alpha) in row.iter().enumerate() {
                            let px = glyph_x + img_x as i32 + image.placement.left;
                            let py = glyph_y + img_y as i32 - image.placement.top;

                            if px >= 0
                                && py >= 0
                                && px < pixmap.width() as i32
                                && py < pixmap.height() as i32
                            {
                                let existing = pixmap.pixel(px as u32, py as u32).unwrap();

                                // Alpha blend
                                let alpha_f = alpha as f32 / 255.0;
                                let r = (color.r as f32 * alpha_f
                                    + existing.red() as f32 * (1.0 - alpha_f))
                                    as u8;
                                let g = (color.g as f32 * alpha_f
                                    + existing.green() as f32 * (1.0 - alpha_f))
                                    as u8;
                                let b = (color.b as f32 * alpha_f
                                    + existing.blue() as f32 * (1.0 - alpha_f))
                                    as u8;

                                let blended = tiny_skia::ColorU8::from_rgba(r, g, b, 255);
                                let width = pixmap.width();
                                pixmap.pixels_mut()[(py as u32 * width + px as u32) as usize] =
                                    blended.premultiply();
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}
