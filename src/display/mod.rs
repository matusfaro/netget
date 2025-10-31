//! Display module - Protocol-agnostic image generation for NetGet
//!
//! This module provides LLM-controlled image rendering capabilities that can be used
//! by any protocol requiring visual output (VNC, HTTP image serving, IPP print previews, etc.).
//!
//! ## Architecture
//!
//! - `types` - Core types (Color, Point, Rect, DisplayCommand)
//! - `canvas` - Drawing operations using tiny-skia
//! - `text` - Text rendering using cosmic-text
//! - `ascii` - ASCII art rendering
//!
//! ## Usage Example
//!
//! ```rust,ignore
//! use netget::display::{DisplayCanvas, DisplayCommand, Color};
//!
//! let mut canvas = DisplayCanvas::new(800, 600);
//! canvas.add_command(DisplayCommand::SetBackground {
//!     color: Color::rgb(50, 50, 50)
//! });
//! canvas.add_command(DisplayCommand::DrawText {
//!     x: 100,
//!     y: 100,
//!     text: "Hello, World!".to_string(),
//!     font_size: 24,
//!     color: Color::rgb(255, 255, 255),
//! });
//!
//! let image_buffer = canvas.render();
//! ```

pub mod types;
pub mod canvas;
pub mod text;
pub mod ascii;

pub use types::{Color, Point, Rect, DisplayCommand};
pub use canvas::DisplayCanvas;
pub use text::TextRenderer;
pub use ascii::AsciiRenderer;
