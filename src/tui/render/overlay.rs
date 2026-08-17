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
        Modal::ProtocolPicker {
            entries,
            filter,
            selected,
            ..
        } => picker_lines(app, entries, filter, *selected),
        Modal::Form(form) => form_lines(app, form),
        Modal::TextEditor { editor, .. } => {
            // The textarea widget renders itself; draw it after the chrome.
            let _ = editor;
            Vec::new()
        }
        Modal::Composer(composer) => composer_lines(app, composer),
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

    // The text editor is a live widget rather than static lines.
    if let Some(Modal::TextEditor { editor, .. }) = app.modals.last() {
        let rows = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(1),
                ratatui::layout::Constraint::Min(1),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(inner);
        frame.render_widget(
            Paragraph::new(Span::styled(editor.help.clone(), app.styles.dimmed)),
            rows[0],
        );
        frame.render_widget(&editor.textarea, rows[1]);
        if let Some(error) = &editor.error {
            frame.render_widget(
                Paragraph::new(Span::styled(error.clone(), app.styles.error)),
                rows[2],
            );
        }
        return;
    }

    let paragraph = Paragraph::new(body_lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, inner);
}

fn picker_lines<'a>(
    app: &DashboardApp,
    entries: &[crate::tui::modal::protocol_picker::ProtocolEntry],
    filter: &str,
    selected: usize,
) -> Vec<Line<'a>> {
    use crate::tui::modal::protocol_picker;
    let matches = protocol_picker::filter(entries, filter);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("filter: ", app.styles.dimmed),
            Span::styled(filter.to_string(), app.styles.accent),
            Span::styled(
                format!("   {} of {} protocols", matches.len(), entries.len()),
                app.styles.dimmed,
            ),
        ]),
        Line::from(""),
    ];
    for (index, entry) in matches.iter().enumerate() {
        let style = if index == selected {
            app.styles.selected
        } else {
            app.styles.normal
        };
        let badge_style = match entry.state {
            crate::protocol::metadata::DevelopmentState::Beta => app.styles.success,
            crate::protocol::metadata::DevelopmentState::Stable => app.styles.success,
            _ => app.styles.warning,
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<22}", entry.name), style),
            Span::styled(format!("{:<12}", entry.badge()), badge_style),
            Span::styled(
                crate::utils::truncate_for_log(&entry.description, 70),
                app.styles.dimmed,
            ),
        ]));
        if index == selected {
            if let Some(note) = &entry.privilege_note {
                lines.push(Line::from(Span::styled(
                    format!("      ⚠ {note}"),
                    app.styles.warning,
                )));
            }
            if !entry.has_binding_defaults {
                lines.push(Line::from(Span::styled(
                    "      declares no default binding — a port is required".to_string(),
                    app.styles.dimmed,
                )));
            }
            if let Some(notes) = &entry.notes {
                lines.push(Line::from(Span::styled(
                    format!("      {}", crate::utils::truncate_for_log(notes, 90)),
                    app.styles.dimmed,
                )));
            }
        }
    }
    if matches.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no protocol matches that filter)",
            app.styles.dimmed,
        )));
    }
    lines
}

fn form_lines<'a>(
    app: &DashboardApp,
    form: &crate::tui::modal::form::FormModel,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    for (index, field) in form.fields.iter().enumerate() {
        let is_selected = index == form.selected;
        let marker = if is_selected { "▸ " } else { "  " };
        let label_style = if is_selected {
            app.styles.accent
        } else {
            app.styles.normal
        };
        let value_display = if is_selected && form.editing.is_some() {
            format!("{}_", form.editing.as_deref().unwrap_or(""))
        } else if field.value.is_empty() {
            field.placeholder.clone()
        } else if field.multiline {
            let first = field.value.lines().next().unwrap_or("");
            let extra = field.value.lines().count().saturating_sub(1);
            if extra > 0 {
                format!("{first} … (+{extra} lines)")
            } else {
                first.to_string()
            }
        } else {
            field.value.clone()
        };
        let value_style = if field.value.is_empty() && form.editing.is_none() {
            app.styles.dimmed
        } else {
            app.styles.info
        };
        lines.push(Line::from(vec![
            Span::styled(marker, app.styles.accent),
            Span::styled(
                format!("{:<22}", format!("{}{}", field.label, if field.required { " *" } else { "" })),
                label_style,
            ),
            Span::styled(value_display, value_style),
        ]));
        if is_selected {
            lines.push(Line::from(Span::styled(
                format!("    {}", field.help),
                app.styles.dimmed,
            )));
        }
    }
    if let Some(error) = &form.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("✗ {error}"),
            app.styles.error,
        )));
    }
    lines
}

fn composer_lines<'a>(
    app: &DashboardApp,
    composer: &crate::tui::modal::composer::ComposerModel,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    match composer.chosen {
        None => {
            lines.push(Line::from(Span::styled(
                format!("{} actions — pick one:", composer.protocol),
                app.styles.dimmed,
            )));
            lines.push(Line::from(""));
            for (index, action) in composer.actions.iter().enumerate() {
                let style = if index == composer.selected {
                    app.styles.selected
                } else {
                    app.styles.normal
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:<24}", action.name), style),
                    Span::styled(
                        crate::utils::truncate_for_log(&action.description, 70),
                        app.styles.dimmed,
                    ),
                ]));
            }
        }
        Some(index) => {
            let action = &composer.actions[index];
            lines.push(Line::from(Span::styled(
                format!("{} — {}", action.name, action.description),
                app.styles.accent,
            )));
            lines.push(Line::from(""));
            if let Some(raw) = &composer.raw_json {
                lines.push(Line::from(Span::styled(
                    "raw JSON (Ctrl-J to go back to fields):",
                    app.styles.dimmed,
                )));
                for line in raw.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        app.styles.info,
                    )));
                }
            } else {
                for (i, field) in composer.fields.iter().enumerate() {
                    let is_selected = i == composer.selected;
                    let marker = if is_selected { "▸ " } else { "  " };
                    let value = if is_selected && composer.editing.is_some() {
                        format!("{}_", composer.editing.as_deref().unwrap_or(""))
                    } else if field.value.is_empty() {
                        field.placeholder.clone()
                    } else {
                        field.value.clone()
                    };
                    lines.push(Line::from(vec![
                        Span::styled(marker, app.styles.accent),
                        Span::styled(
                            format!(
                                "{:<20}",
                                format!("{}{}", field.name, if field.required { " *" } else { "" })
                            ),
                            if is_selected {
                                app.styles.accent
                            } else {
                                app.styles.normal
                            },
                        ),
                        Span::styled(
                            value,
                            if field.value.is_empty() {
                                app.styles.dimmed
                            } else {
                                app.styles.info
                            },
                        ),
                    ]));
                    if is_selected {
                        lines.push(Line::from(Span::styled(
                            format!("    {}", field.help),
                            app.styles.dimmed,
                        )));
                    }
                }
                if composer.fields.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "(this action takes no parameters — Ctrl-S to send)",
                        app.styles.dimmed,
                    )));
                }
            }
        }
    }
    if let Some(error) = &composer.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("✗ {error}"),
            app.styles.error,
        )));
    }
    lines
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
