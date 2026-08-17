//! Modal chrome: a centred, cleared box with a title, scrollable body and a
//! hint line.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::app::DashboardApp;
use crate::tui::hit::HitTarget;
use crate::tui::modal::{help, request_detail, Modal};

pub fn draw(frame: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let Some(modal) = app.modals.last() else {
        return;
    };
    let (pw, ph) = modal.size_percent();
    let rect = super::centered(area, pw, ph);
    let title = modal.title();
    let hint = modal.hint();

    let body_lines: Vec<Line> = match modal {
        Modal::Help { .. } => help::help_lines()
            .into_iter()
            .map(|(key, text)| match key {
                Some(key) => Line::from(vec![
                    Span::styled(format!("  {key:<22}"), app.styles.accent),
                    Span::styled(text.to_string(), app.styles.normal),
                ]),
                None => Line::from(Span::styled(
                    format!("\n{text}"),
                    app.styles.title,
                )),
            })
            .collect(),
        Modal::RequestDetail { entry, .. } => request_detail::detail_lines(entry)
            .into_iter()
            .map(|l| Line::from(Span::styled(l, app.styles.normal)))
            .collect(),
        Modal::Confirm { message, .. } => vec![
            Line::from(Span::styled(message.clone(), app.styles.warning)),
            Line::from(""),
            Line::from(Span::styled(
                "This cannot be undone.",
                app.styles.dimmed,
            )),
        ],
        Modal::WebApproval { url, .. } => vec![
            Line::from(Span::styled(
                "The model wants to search the web:",
                app.styles.normal,
            )),
            Line::from(""),
            Line::from(Span::styled(url.clone(), app.styles.info)),
        ],
        Modal::BandDetail { key, .. } => band_detail_lines(app, *key),
    };

    let scroll = match modal {
        Modal::Help { scroll }
        | Modal::RequestDetail { scroll, .. }
        | Modal::BandDetail { scroll, .. } => *scroll,
        _ => 0,
    };

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(app.styles.accent)
        .title(Span::styled(format!(" {title} "), app.styles.title))
        .title_bottom(Span::styled(format!(" {hint} "), app.styles.dimmed));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    app.hits.push(rect, HitTarget::ModalBody);

    let paragraph = Paragraph::new(body_lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, inner);
}

fn band_detail_lines<'a>(
    app: &DashboardApp,
    key: crate::tui::app::UiKey,
) -> Vec<Line<'a>> {
    use crate::tui::app::UiKey;
    let mut lines = Vec::new();
    let mut push = |text: String, style| lines.push(Line::from(Span::styled(text, style)));
    match key {
        UiKey::Server(id) => {
            if let Some(row) = app.snapshot.servers.iter().find(|s| s.id == id) {
                push(format!("protocol   {}", row.protocol), app.styles.normal);
                push(
                    format!(
                        "address    {}",
                        row.local_addr.clone().unwrap_or_else(|| format!(":{}", row.port))
                    ),
                    app.styles.normal,
                );
                push(format!("status     {}", row.status), app.styles.normal);
                push(String::new(), app.styles.normal);
                push("instruction".to_string(), app.styles.title);
                for line in row.instruction.lines() {
                    push(format!("  {line}"), app.styles.dimmed);
                }
                push(String::new(), app.styles.normal);
                push("startup_params".to_string(), app.styles.title);
                let params = row
                    .startup_params
                    .as_ref()
                    .map(|p| serde_json::to_string_pretty(p).unwrap_or_default())
                    .unwrap_or_else(|| "(none)".to_string());
                for line in params.lines() {
                    push(format!("  {line}"), app.styles.dimmed);
                }
                push(String::new(), app.styles.normal);
                push("routing".to_string(), app.styles.title);
                let routing = row
                    .routing
                    .as_ref()
                    .map(|r| serde_json::to_string_pretty(&r.handlers).unwrap_or_default())
                    .unwrap_or_else(|| "(none — every event goes to the LLM)".to_string());
                for line in routing.lines() {
                    push(format!("  {line}"), app.styles.dimmed);
                }
            }
        }
        UiKey::Client(id) => {
            if let Some(row) = app.snapshot.clients.iter().find(|c| c.id == id) {
                push(format!("protocol   {}", row.protocol), app.styles.normal);
                push(format!("remote     {}", row.remote_addr), app.styles.normal);
                push(format!("status     {}", row.status), app.styles.normal);
                push(
                    format!(
                        "send       {}",
                        if row.sendable {
                            "supported (press n)"
                        } else {
                            "not supported by this protocol yet"
                        }
                    ),
                    app.styles.dimmed,
                );
                push(String::new(), app.styles.normal);
                push("instruction".to_string(), app.styles.title);
                for line in row.instruction.lines() {
                    push(format!("  {line}"), app.styles.dimmed);
                }
                push(String::new(), app.styles.normal);
                push("routing".to_string(), app.styles.title);
                let routing = row
                    .routing
                    .as_ref()
                    .map(|r| serde_json::to_string_pretty(&r.handlers).unwrap_or_default())
                    .unwrap_or_else(|| "(none — every event goes to the LLM)".to_string());
                for line in routing.lines() {
                    push(format!("  {line}"), app.styles.dimmed);
                }
            }
        }
    }
    lines
}
