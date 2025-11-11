//! Canvas implementation using tiny-skia for 2D graphics rendering

use crate::display::ascii::AsciiRenderer;
use crate::display::text::TextRenderer;
use crate::display::types::{Color, DisplayCommand};
use image::{ImageBuffer, Rgb};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Display canvas that accumulates drawing commands and renders to an image buffer
pub struct DisplayCanvas {
    width: u32,
    height: u32,
    commands: Vec<DisplayCommand>,
}

impl DisplayCanvas {
    /// Create a new canvas with the specified dimensions
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            commands: Vec::new(),
        }
    }

    /// Add a drawing command to the canvas
    pub fn add_command(&mut self, cmd: DisplayCommand) {
        self.commands.push(cmd);
    }

    /// Add multiple drawing commands to the canvas
    pub fn add_commands(&mut self, cmds: Vec<DisplayCommand>) {
        self.commands.extend(cmds);
    }

    /// Clear all drawing commands
    pub fn clear_commands(&mut self) {
        self.commands.clear();
    }

    /// Render all commands to an RGB image buffer
    pub fn render(&self) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
        // Create tiny-skia pixmap
        let mut pixmap = Pixmap::new(self.width, self.height).expect("Failed to create pixmap");

        // Execute all drawing commands
        for cmd in &self.commands {
            self.execute_command(&mut pixmap, cmd);
        }

        // Convert pixmap to image buffer
        pixmap_to_image_buffer(&pixmap)
    }

    /// Execute a single drawing command on the pixmap
    fn execute_command(&self, pixmap: &mut Pixmap, cmd: &DisplayCommand) {
        match cmd {
            DisplayCommand::SetBackground { color } => {
                self.set_background(pixmap, *color);
            }
            DisplayCommand::Clear => {
                pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 0, 255));
            }
            DisplayCommand::DrawRectangle {
                x,
                y,
                width,
                height,
                color,
                filled,
            } => {
                self.draw_rectangle(pixmap, *x, *y, *width, *height, *color, *filled);
            }
            DisplayCommand::DrawLine {
                x1,
                y1,
                x2,
                y2,
                color,
                width,
            } => {
                self.draw_line(pixmap, *x1, *y1, *x2, *y2, *color, *width);
            }
            DisplayCommand::DrawCircle {
                x,
                y,
                radius,
                color,
                filled,
            } => {
                self.draw_circle(pixmap, *x, *y, *radius, *color, *filled);
            }
            DisplayCommand::DrawText {
                x,
                y,
                text,
                font_size,
                color,
            } => {
                let mut text_renderer = TextRenderer::new();
                text_renderer.draw_text(pixmap, *x, *y, text, *font_size, *color);
            }
            DisplayCommand::RenderAsciiArt {
                text,
                font_size,
                fg_color,
                bg_color,
            } => {
                let mut ascii_renderer = AsciiRenderer::new();
                ascii_renderer.render(pixmap, text, *font_size, *fg_color, *bg_color);
            }
            DisplayCommand::DrawWindow {
                x,
                y,
                width,
                height,
                title,
                content,
            } => {
                self.draw_window(pixmap, *x, *y, *width, *height, title, content);
            }
            DisplayCommand::DrawButton {
                x,
                y,
                width,
                height,
                label,
            } => {
                self.draw_button(pixmap, *x, *y, *width, *height, label);
            }
            DisplayCommand::DrawTextBox {
                x,
                y,
                width,
                height,
                text,
                placeholder,
            } => {
                self.draw_textbox(pixmap, *x, *y, *width, *height, text, placeholder);
            }
        }
    }

    fn set_background(&self, pixmap: &mut Pixmap, color: Color) {
        let sk_color = color_to_tiny_skia(color);
        pixmap.fill(sk_color);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_rectangle(
        &self,
        pixmap: &mut Pixmap,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        color: Color,
        filled: bool,
    ) {
        let mut path_builder = PathBuilder::new();
        path_builder.push_rect(
            tiny_skia::Rect::from_xywh(x as f32, y as f32, width as f32, height as f32).unwrap(),
        );
        let path = path_builder.finish().unwrap();

        let mut paint = Paint::default();
        paint.set_color(color_to_tiny_skia(color));
        paint.anti_alias = true;

        if filled {
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        } else {
            let stroke = Stroke {
                width: 1.0,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_line(
        &self,
        pixmap: &mut Pixmap,
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        color: Color,
        width: u32,
    ) {
        let mut path_builder = PathBuilder::new();
        path_builder.move_to(x1 as f32, y1 as f32);
        path_builder.line_to(x2 as f32, y2 as f32);
        let path = path_builder.finish().unwrap();

        let mut paint = Paint::default();
        paint.set_color(color_to_tiny_skia(color));
        paint.anti_alias = true;

        let stroke = Stroke {
            width: width as f32,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    fn draw_circle(
        &self,
        pixmap: &mut Pixmap,
        x: u32,
        y: u32,
        radius: u32,
        color: Color,
        filled: bool,
    ) {
        let mut path_builder = PathBuilder::new();
        path_builder.push_circle(x as f32, y as f32, radius as f32);
        let path = path_builder.finish().unwrap();

        let mut paint = Paint::default();
        paint.set_color(color_to_tiny_skia(color));
        paint.anti_alias = true;

        if filled {
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        } else {
            let stroke = Stroke {
                width: 1.0,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_window(
        &self,
        pixmap: &mut Pixmap,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        title: &str,
        content: &[DisplayCommand],
    ) {
        // Draw window background
        self.draw_rectangle(pixmap, x, y, width, height, Color::LIGHT_GRAY, true);

        // Draw window border
        self.draw_rectangle(pixmap, x, y, width, height, Color::DARK_GRAY, false);

        // Draw title bar
        self.draw_rectangle(pixmap, x, y, width, 30, Color::BLUE, true);

        // Draw title text
        let mut text_renderer = TextRenderer::new();
        text_renderer.draw_text(pixmap, x + 10, y + 20, title, 14, Color::WHITE);

        // Draw content (offset by title bar height)
        let content_y = y + 30;
        for cmd in content {
            // Offset content commands relative to window position
            let offset_cmd = offset_command(cmd, x, content_y);
            self.execute_command(pixmap, &offset_cmd);
        }
    }

    fn draw_button(
        &self,
        pixmap: &mut Pixmap,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        label: &str,
    ) {
        // Draw button background
        self.draw_rectangle(pixmap, x, y, width, height, Color::GRAY, true);

        // Draw button border
        self.draw_rectangle(pixmap, x, y, width, height, Color::BLACK, false);

        // Draw button label (centered)
        let mut text_renderer = TextRenderer::new();
        let label_x = x + (width / 2).saturating_sub((label.len() as u32 * 7) / 2);
        let label_y = y + (height / 2) + 5;
        text_renderer.draw_text(pixmap, label_x, label_y, label, 14, Color::BLACK);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_textbox(
        &self,
        pixmap: &mut Pixmap,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        text: &str,
        placeholder: &Option<String>,
    ) {
        // Draw textbox background
        self.draw_rectangle(pixmap, x, y, width, height, Color::WHITE, true);

        // Draw textbox border
        self.draw_rectangle(pixmap, x, y, width, height, Color::GRAY, false);

        // Draw text or placeholder
        let mut text_renderer = TextRenderer::new();
        if text.is_empty() {
            if let Some(ph) = placeholder {
                text_renderer.draw_text(pixmap, x + 5, y + (height / 2) + 5, ph, 14, Color::GRAY);
            }
        } else {
            text_renderer.draw_text(pixmap, x + 5, y + (height / 2) + 5, text, 14, Color::BLACK);
        }
    }
}

/// Convert a Color to tiny-skia Color
fn color_to_tiny_skia(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a)
}

/// Convert tiny-skia Pixmap to image::ImageBuffer
fn pixmap_to_image_buffer(pixmap: &Pixmap) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let width = pixmap.width();
    let height = pixmap.height();
    let mut img_buf = ImageBuffer::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let pixel = pixmap.pixel(x, y).unwrap();
            img_buf.put_pixel(x, y, Rgb([pixel.red(), pixel.green(), pixel.blue()]));
        }
    }

    img_buf
}

/// Offset a display command by a given position (for window content)
fn offset_command(cmd: &DisplayCommand, offset_x: u32, offset_y: u32) -> DisplayCommand {
    match cmd {
        DisplayCommand::DrawRectangle {
            x,
            y,
            width,
            height,
            color,
            filled,
        } => DisplayCommand::DrawRectangle {
            x: x + offset_x,
            y: y + offset_y,
            width: *width,
            height: *height,
            color: *color,
            filled: *filled,
        },
        DisplayCommand::DrawText {
            x,
            y,
            text,
            font_size,
            color,
        } => DisplayCommand::DrawText {
            x: x + offset_x,
            y: y + offset_y,
            text: text.clone(),
            font_size: *font_size,
            color: *color,
        },
        DisplayCommand::DrawLine {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        } => DisplayCommand::DrawLine {
            x1: x1 + offset_x,
            y1: y1 + offset_y,
            x2: x2 + offset_x,
            y2: y2 + offset_y,
            color: *color,
            width: *width,
        },
        DisplayCommand::DrawCircle {
            x,
            y,
            radius,
            color,
            filled,
        } => DisplayCommand::DrawCircle {
            x: x + offset_x,
            y: y + offset_y,
            radius: *radius,
            color: *color,
            filled: *filled,
        },
        DisplayCommand::DrawButton {
            x,
            y,
            width,
            height,
            label,
        } => DisplayCommand::DrawButton {
            x: x + offset_x,
            y: y + offset_y,
            width: *width,
            height: *height,
            label: label.clone(),
        },
        DisplayCommand::DrawTextBox {
            x,
            y,
            width,
            height,
            text,
            placeholder,
        } => DisplayCommand::DrawTextBox {
            x: x + offset_x,
            y: y + offset_y,
            width: *width,
            height: *height,
            text: text.clone(),
            placeholder: placeholder.clone(),
        },
        DisplayCommand::DrawWindow {
            x,
            y,
            width,
            height,
            title,
            content,
        } => DisplayCommand::DrawWindow {
            x: x + offset_x,
            y: y + offset_y,
            width: *width,
            height: *height,
            title: title.clone(),
            content: content.clone(),
        },
        // Commands that don't need offsetting
        DisplayCommand::SetBackground { color } => DisplayCommand::SetBackground { color: *color },
        DisplayCommand::Clear => DisplayCommand::Clear,
        DisplayCommand::RenderAsciiArt {
            text,
            font_size,
            fg_color,
            bg_color,
        } => DisplayCommand::RenderAsciiArt {
            text: text.clone(),
            font_size: *font_size,
            fg_color: *fg_color,
            bg_color: *bg_color,
        },
    }
}
