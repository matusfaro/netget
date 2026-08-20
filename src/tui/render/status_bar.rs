//! Bottom status bar: the indicators the legacy sticky footer carried, each
//! clickable to open or cycle the thing it names.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::DashboardApp;
use crate::tui::hit::{HitTarget, SegmentId};

pub fn draw(frame: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let mut segments: Vec<(String, SegmentId)> = Vec::new();
    segments.push((
        format!(
            " {} ",
            if app.status.model.is_empty() {
                "no model".to_string()
            } else {
                app.status.model.clone()
            }
        ),
        SegmentId::Model,
    ));
    if !app.status.backend.is_empty() {
        segments.push((format!(" {} ", app.status.backend), SegmentId::Backend));
    }
    segments.push((
        format!(" log:{} ^L ", app.core.log_level.as_str()),
        SegmentId::LogLevel,
    ));
    segments.push((
        format!(" web:{} ^W ", app.status.web_search),
        SegmentId::WebSearch,
    ));
    segments.push((
        format!(" handler:{} ^H ", app.status.handler_mode),
        SegmentId::Handler,
    ));
    segments.push((
        format!(" scr:{} ^E ", app.status.scripting),
        SegmentId::Scripting,
    ));
    if app.status.llm_calls > 0 {
        segments.push((
            format!(
                " tok {}k/{}k · {} calls ",
                app.status.input_tokens / 1000,
                app.status.output_tokens / 1000,
                app.status.llm_calls
            ),
            SegmentId::Usage,
        ));
    }
    if app.status.active_conversations > 0 {
        segments.push((
            format!(" llm:{} ", app.status.active_conversations),
            SegmentId::Usage,
        ));
    }
    if let Some(notice) = &app.status.notice {
        segments.push((format!(" {notice} "), SegmentId::Usage));
    }
    segments.push((" F1 keys ".to_string(), SegmentId::Help));

    let mut spans: Vec<Span> = Vec::new();
    let mut x = area.x;
    for (index, (text, id)) in segments.iter().enumerate() {
        let width = text.chars().count() as u16;
        if x + width > area.x + area.width {
            break;
        }
        if index > 0 {
            spans.push(Span::styled("│", app.styles.separator));
            x += 1;
        }
        spans.push(Span::styled(text.clone(), app.styles.dimmed));
        app.hits.push(
            Rect {
                x,
                y: area.y,
                width,
                height: 1,
            },
            HitTarget::StatusSegment(*id),
        );
        x += width;
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
