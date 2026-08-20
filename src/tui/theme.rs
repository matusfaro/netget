//! Adapter from the shared crossterm `ColorPalette` (with termbg
//! auto-detection) to ratatui `Style`s. ratatui 0.29 converts
//! `crossterm::style::Color` losslessly via `From`.

use ratatui::style::{Color, Modifier, Style};

use crate::cli::theme::ColorPalette;

#[derive(Clone)]
pub struct Styles {
    pub error: Style,
    pub warning: Style,
    pub info: Style,
    pub debug: Style,
    pub trace: Style,
    pub reasoning: Style,
    pub user: Style,
    pub server: Style,
    pub client: Style,
    pub connection: Style,
    pub separator: Style,
    pub dimmed: Style,
    pub normal: Style,
    pub success: Style,
    pub failure: Style,
    pub ask: Style,
    /// Accent for focused borders, buttons, selection gutters.
    pub accent: Style,
    pub selected: Style,
    pub button: Style,
    pub title: Style,
}

impl Styles {
    pub fn from_palette(palette: &ColorPalette) -> Self {
        let c = |color: crossterm::style::Color| -> Color { Color::from(color) };
        let accent = c(palette.info);
        Self {
            error: Style::default().fg(c(palette.error)),
            warning: Style::default().fg(c(palette.warning)),
            info: Style::default().fg(c(palette.info)),
            debug: Style::default().fg(c(palette.debug)),
            trace: Style::default().fg(c(palette.trace)),
            reasoning: Style::default().fg(c(palette.reasoning)),
            user: Style::default().fg(c(palette.user)).add_modifier(Modifier::BOLD),
            server: Style::default().fg(c(palette.server)),
            client: Style::default().fg(c(palette.client)),
            connection: Style::default().fg(c(palette.connection)),
            separator: Style::default().fg(c(palette.separator)),
            dimmed: Style::default().fg(c(palette.dimmed)),
            normal: Style::default().fg(c(palette.normal)),
            success: Style::default().fg(c(palette.success)),
            failure: Style::default().fg(c(palette.failure)),
            ask: Style::default().fg(c(palette.ask)),
            accent: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            selected: Style::default().add_modifier(Modifier::REVERSED),
            button: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            title: Style::default().add_modifier(Modifier::BOLD),
        }
    }

    pub fn for_log_level(&self, level: crate::ui::app::LogLevel) -> Style {
        use crate::ui::app::LogLevel;
        match level {
            LogLevel::Error => self.error,
            LogLevel::Warn => self.warning,
            LogLevel::Info => self.info,
            LogLevel::Debug => self.debug,
            LogLevel::Trace => self.trace,
        }
    }
}
