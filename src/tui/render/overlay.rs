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
        // Rendered separately below (list + fixed detail pane).
        Modal::ProtocolPicker { .. } => Vec::new(),
        Modal::Form(form) => form_lines(app, form),
        Modal::TextEditor { editor, .. } => {
            // The textarea widget renders itself; draw it after the chrome.
            let _ = editor;
            Vec::new()
        }
        Modal::Composer(composer) => composer_lines(app, composer),
        Modal::Routing(model) => routing_lines(app, model),
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

    // The instance form: fields, then Apply / Cancel.
    if let Some(Modal::Form(form)) = app.modals.last() {
        let rows = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(3),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(inner);
        let lines = form_lines(app, form);
        let buttons = form.buttons();
        let focused = form.focused_action();
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            rows[0],
        );
        draw_button_row(frame, app, rows[1], &buttons, focused);
        return;
    }

    // The routing editor: list, then a row of real buttons, then the note
    // about the implicit fallback.
    if let Some(Modal::Routing(model)) = app.modals.last() {
        if let Some(draft) = &model.draft {
            let rows = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Min(3),
                    ratatui::layout::Constraint::Length(1),
                ])
                .split(inner);
            let lines = routing_lines(app, model);
            let buttons = draft.buttons();
            let focused = draft.focused_action();
            frame.render_widget(
                Paragraph::new(lines).wrap(Wrap { trim: false }),
                rows[0],
            );
            draw_button_row(frame, app, rows[1], &buttons, focused);
            return;
        }
        if model.draft.is_none() {
            use crate::tui::modal::routing::RoutingFocus;

            let rows = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Min(3),
                    ratatui::layout::Constraint::Length(1),
                    ratatui::layout::Constraint::Length(2),
                ])
                .split(inner);

            // --- handler list ---
            let mut lines: Vec<Line> = vec![Line::from(Span::styled(
                "Handlers are matched in order; the first match wins.",
                app.styles.dimmed,
            ))];
            let handler_rows = model.rows();
            if handler_rows.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (none yet — Add one to answer an event without the model)",
                    app.styles.dimmed,
                )));
            }
            for (index, row) in handler_rows.iter().enumerate() {
                let selected = model.focus == RoutingFocus::List && index == model.selected;
                lines.push(Line::from(vec![
                    Span::styled(
                        if selected { "▸ " } else { "  " },
                        app.styles.accent,
                    ),
                    Span::styled(
                        row.clone(),
                        if selected {
                            app.styles.selected
                        } else {
                            app.styles.normal
                        },
                    ),
                ]));
            }
            if let Some(error) = &model.error {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("✗ {error}"),
                    app.styles.error,
                )));
            }
            frame.render_widget(Paragraph::new(lines), rows[0]);

            // --- buttons ---
            let buttons = model.buttons();
            let focused = model.focused_button();
            let mut spans: Vec<Span> = Vec::new();
            let mut x = rows[1].x;
            for action in &buttons {
                let label = action.label();
                let width = label.chars().count() as u16;
                if x + width > rows[1].x + rows[1].width {
                    break;
                }
                let is_focused = focused == Some(*action);
                spans.push(Span::styled(
                    label,
                    if is_focused {
                        app.styles.selected
                    } else {
                        app.styles.button
                    },
                ));
                spans.push(Span::raw(" "));
                app.hits.push(
                    ratatui::layout::Rect {
                        x,
                        y: rows[1].y,
                        width,
                        height: 1,
                    },
                    HitTarget::ModalActionButton(*action),
                );
                x += width + 1;
            }
            frame.render_widget(Paragraph::new(Line::from(spans)), rows[1]);

            // --- the implicit fallback, stated rather than faked as a row ---
            frame.render_widget(
                Paragraph::new(Span::styled(model.fallback_note(), app.styles.dimmed))
                    .wrap(Wrap { trim: false }),
                rows[2],
            );
            return;
        }
    }

    // The picker is a list with a fixed detail pane beneath it.
    if let Some(Modal::ProtocolPicker {
        entries,
        filter,
        selected,
        ..
    }) = app.modals.last()
    {
        const DETAIL_HEIGHT: u16 = 7;
        let rows = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(3),
                ratatui::layout::Constraint::Length(DETAIL_HEIGHT),
            ])
            .split(inner);

        use crate::tui::modal::protocol_picker;
        let matches = protocol_picker::filter(entries, filter);

        // One header row (the filter), then a window over the matches sized to
        // what is left. Scrolling here is not optional: with every protocol
        // compiled in the list is far longer than the modal, and a selection
        // that runs off the bottom is invisible.
        let header = Line::from(vec![
            Span::styled("filter: ", app.styles.dimmed),
            Span::styled(filter.to_string(), app.styles.accent),
            Span::styled(
                format!("   {} of {} protocols", matches.len(), entries.len()),
                app.styles.dimmed,
            ),
        ]);
        let visible = rows[0].height.saturating_sub(1) as usize;
        let offset = if matches.is_empty() || visible == 0 {
            0
        } else {
            // Keep the selection inside the window, biased to show context
            // above it once we are scrolled down.
            selected.saturating_sub(visible.saturating_sub(1))
        };

        let mut list = vec![header];
        if matches.is_empty() {
            list.push(Line::from(Span::styled(
                "  (no protocol matches that filter)",
                app.styles.dimmed,
            )));
        }
        for (index, entry) in matches.iter().enumerate().skip(offset).take(visible) {
            let style = if index == *selected {
                app.styles.selected
            } else {
                app.styles.normal
            };
            let badge_style = match entry.state {
                crate::protocol::metadata::DevelopmentState::Beta
                | crate::protocol::metadata::DevelopmentState::Stable => app.styles.success,
                _ => app.styles.warning,
            };
            list.push(Line::from(vec![
                Span::styled(format!("  {:<22}", entry.name), style),
                Span::styled(format!("{:<12}", entry.badge()), badge_style),
                Span::styled(
                    crate::utils::truncate_for_log(&entry.description, 70),
                    app.styles.dimmed,
                ),
            ]));
        }
        frame.render_widget(Paragraph::new(list), rows[0]);

        let detail = Block::default()
            .borders(Borders::TOP)
            .border_style(app.styles.separator);
        let detail_inner = detail.inner(rows[1]);
        frame.render_widget(detail, rows[1]);
        frame.render_widget(
            Paragraph::new(picker_detail_lines(app, entries, filter, *selected))
                .wrap(Wrap { trim: false }),
            detail_inner,
        );
        return;
    }

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

/// Detail for the highlighted protocol, rendered in a fixed pane so the list
/// above it never moves.
/// Draw a row of modal buttons, registering each as a hit target.
fn draw_button_row(
    frame: &mut ratatui::Frame,
    app: &mut DashboardApp,
    area: ratatui::layout::Rect,
    buttons: &[crate::tui::hit::ModalAction],
    focused: Option<crate::tui::hit::ModalAction>,
) {
    let mut spans: Vec<Span> = Vec::new();
    let mut x = area.x;
    for action in buttons {
        let label = action.label();
        let width = label.chars().count() as u16;
        if x + width > area.x + area.width {
            break;
        }
        spans.push(Span::styled(
            label,
            if focused == Some(*action) {
                app.styles.selected
            } else {
                app.styles.button
            },
        ));
        spans.push(Span::raw(" "));
        app.hits.push(
            ratatui::layout::Rect {
                x,
                y: area.y,
                width,
                height: 1,
            },
            HitTarget::ModalActionButton(*action),
        );
        x += width + 1;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn picker_detail_lines<'a>(
    app: &DashboardApp,
    entries: &[crate::tui::modal::protocol_picker::ProtocolEntry],
    filter: &str,
    selected: usize,
) -> Vec<Line<'a>> {
    use crate::tui::modal::protocol_picker;
    let matches = protocol_picker::filter(entries, filter);
    let Some(entry) = matches.get(selected) else {
        return vec![Line::from(Span::styled(
            "(no protocol selected)",
            app.styles.dimmed,
        ))];
    };

    let mut lines = vec![Line::from(vec![
        Span::styled(entry.name.clone(), app.styles.accent),
        Span::styled(format!("  {}", entry.badge()), app.styles.dimmed),
    ])];
    lines.push(Line::from(Span::styled(
        entry.description.clone(),
        app.styles.normal,
    )));
    lines.push(Line::from(Span::styled(
        match entry.default_port {
            Some(port) => format!("starts on port {port}"),
            None => "starts on an OS-assigned port (0)".to_string(),
        },
        app.styles.dimmed,
    )));
    if let Some(note) = &entry.privilege_note {
        lines.push(Line::from(Span::styled(
            format!("⚠ {note}"),
            app.styles.warning,
        )));
    }
    if let Some(notes) = &entry.notes {
        lines.push(Line::from(Span::styled(
            crate::utils::truncate_for_log(notes, 200),
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

fn routing_lines<'a>(
    app: &DashboardApp,
    model: &crate::tui::modal::routing::RoutingModel,
) -> Vec<Line<'a>> {
    use crate::tui::modal::routing::{HandlerEditFocus, HandlerKind};

    let mut lines = Vec::new();

    if let Some(draft) = &model.draft {
        let field = |focused: bool| {
            if focused {
                app.styles.accent
            } else {
                app.styles.normal
            }
        };
        lines.push(Line::from(Span::styled(
            "Tab moves between pattern, kind and body.",
            app.styles.dimmed,
        )));
        lines.push(Line::from(""));

        let pattern_display = if draft.editing.is_some() && draft.focus == HandlerEditFocus::Pattern
        {
            format!("{}_", draft.editing.as_deref().unwrap_or(""))
        } else {
            draft.pattern.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(
                if draft.focus == HandlerEditFocus::Pattern { "▸ " } else { "  " },
                app.styles.accent,
            ),
            Span::styled("event pattern    ", field(draft.focus == HandlerEditFocus::Pattern)),
            Span::styled(pattern_display, app.styles.info),
        ]));
        if draft.focus == HandlerEditFocus::Pattern {
            lines.push(Line::from(Span::styled(
                "    '*' matches every event. This protocol raises:",
                app.styles.dimmed,
            )));
            for (id, description) in model.event_ids.iter().take(12) {
                lines.push(Line::from(Span::styled(
                    format!(
                        "      {:<28} {}",
                        id,
                        crate::utils::truncate_for_log(description, 60)
                    ),
                    app.styles.dimmed,
                )));
            }
        }

        lines.push(Line::from(vec![
            Span::styled(
                if draft.focus == HandlerEditFocus::Kind { "▸ " } else { "  " },
                app.styles.accent,
            ),
            Span::styled("handled by       ", field(draft.focus == HandlerEditFocus::Kind)),
            Span::styled(draft.kind.label().to_string(), app.styles.info),
        ]));
        if draft.focus == HandlerEditFocus::Kind {
            lines.push(Line::from(Span::styled(
                "    ←/→ or Enter cycles. Static and script answer without a model call.",
                app.styles.dimmed,
            )));
        }

        let body_label = match draft.kind {
            HandlerKind::Llm => "instruction",
            HandlerKind::Script => "script code",
            HandlerKind::Static => "actions",
        };
        lines.push(Line::from(vec![
            Span::styled(
                if draft.focus == HandlerEditFocus::Body { "▸ " } else { "  " },
                app.styles.accent,
            ),
            Span::styled(
                format!("{body_label:<17}"),
                field(draft.focus == HandlerEditFocus::Body),
            ),
        ]));
        match draft.kind {
            HandlerKind::Llm => {
                let text = if draft.editing.is_some() && draft.focus == HandlerEditFocus::Body {
                    format!("{}_", draft.editing.as_deref().unwrap_or(""))
                } else if draft.instruction.is_empty() {
                    "(press Enter to write the per-event instruction)".to_string()
                } else {
                    draft.instruction.clone()
                };
                lines.push(Line::from(Span::styled(
                    format!("      {text}"),
                    app.styles.info,
                )));
            }
            HandlerKind::Script => {
                lines.push(Line::from(Span::styled(
                    format!("      language: {}", draft.language),
                    app.styles.dimmed,
                )));
                if draft.code.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "      (press Enter to write the script)",
                        app.styles.dimmed,
                    )));
                } else {
                    for line in draft.code.lines().take(8) {
                        lines.push(Line::from(Span::styled(
                            format!("      {line}"),
                            app.styles.info,
                        )));
                    }
                }
            }
            HandlerKind::Static => {
                if draft.actions.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "      (press Enter to build the response actions)",
                        app.styles.dimmed,
                    )));
                } else {
                    for (index, action) in draft.actions.iter().enumerate() {
                        let name = action
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("(no type)");
                        let style = if draft.focus == HandlerEditFocus::Body
                            && index == draft.selected_action
                        {
                            app.styles.selected
                        } else {
                            app.styles.info
                        };
                        lines.push(Line::from(Span::styled(
                            format!("      {name}  {}", crate::utils::truncate_for_log(&action.to_string(), 70)),
                            style,
                        )));
                    }
                }
            }
        }
        if let Some(error) = &draft.error {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("✗ {error}"),
                app.styles.error,
            )));
        }
        return lines;
    }

    lines.push(Line::from(Span::styled(
        "Handlers match in order, first wins:",
        app.styles.dimmed,
    )));
    lines.push(Line::from(""));
    let rows = model.rows();
    let last = rows.len().saturating_sub(1);
    for (index, row) in rows.iter().enumerate() {
        let is_fallback = index == last;
        let style = if is_fallback {
            app.styles.dimmed
        } else if index == model.selected {
            app.styles.selected
        } else {
            app.styles.normal
        };
        let marker = if !is_fallback && index == model.selected {
            "▸ "
        } else {
            "  "
        };
        lines.push(Line::from(vec![
            Span::styled(marker, app.styles.accent),
            Span::styled(row.clone(), style),
        ]));
    }
    if model.handlers.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No handlers: every event goes to the LLM. Press a to add a deterministic one.",
            app.styles.dimmed,
        )));
    }
    if model.dirty {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  unsaved — Ctrl-S applies (hot: no restart, connections kept)",
            app.styles.warning,
        )));
    }
    if let Some(error) = &model.error {
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
                        match row.send_state {
                            crate::tui::projection::SendState::Ready => "available (press n)",
                            crate::tui::projection::SendState::NotConnected => "not connected",
                            crate::tui::projection::SendState::ProtocolUnsupported =>
                                "this protocol has no command channel yet",
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
