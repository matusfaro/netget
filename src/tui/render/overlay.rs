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
                None => Line::from(Span::styled(format!("\n{text}"), app.styles.title)),
            })
            .collect(),
        Modal::RequestDetail { entry, .. } => request_detail::detail_lines(entry)
            .into_iter()
            .map(|l| Line::from(Span::styled(l, app.styles.normal)))
            .collect(),
        Modal::Confirm { message, .. } => vec![
            Line::from(Span::styled(message.clone(), app.styles.warning)),
            Line::from(""),
            Line::from(Span::styled("This cannot be undone.", app.styles.dimmed)),
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
        // Rendered separately below (their own panes and button rows).
        Modal::Composer(_) | Modal::Routing(_) | Modal::Intercept(_) => Vec::new(),
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
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), rows[0]);
        draw_button_row(frame, app, rows[1], &buttons, focused);
        return;
    }

    // The routing editor: either the handler table, or the draft editor with
    // its kind control, its panes and its buttons.
    if let Some(Modal::Routing(model)) = app.modals.last() {
        if let Some(draft) = &model.draft {
            use crate::tui::modal::routing::DraftFocus;
            let kind = draft.kind;
            let kind_focused = draft.focus == DraftFocus::Kind;
            let buttons = draft.buttons();
            let focused = draft.focused_action();
            let body = draft_body_lines(app, model, draft);

            let rows = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Length(1),
                    ratatui::layout::Constraint::Length(2),
                    ratatui::layout::Constraint::Min(3),
                    ratatui::layout::Constraint::Length(1),
                ])
                .split(inner);
            draw_kind_segments(frame, app, rows[0], kind, kind_focused);
            frame.render_widget(
                Paragraph::new(Span::styled(kind.blurb(), app.styles.dimmed))
                    .wrap(Wrap { trim: false }),
                rows[1],
            );
            frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), rows[2]);
            draw_button_row(frame, app, rows[3], &buttons, focused);
            return;
        }

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
            "Rules are matched in order; the first match wins.",
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
                Span::styled(if selected { "▸ " } else { "  " }, app.styles.accent),
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
        let buttons = model.buttons();
        let focused = model.focused_button();
        let note = model.fallback_note();
        frame.render_widget(Paragraph::new(lines), rows[0]);
        draw_button_row(frame, app, rows[1], &buttons, focused);
        frame.render_widget(
            Paragraph::new(Span::styled(note, app.styles.dimmed)).wrap(Wrap { trim: false }),
            rows[2],
        );
        return;
    }

    // A pending intercept: what arrived, the answer being composed, buttons.
    if let Some(Modal::Intercept(model)) = app.modals.last() {
        let buttons = model.buttons();
        let focused = model.focused_action();
        let lines = intercept_lines(app, model);
        let rows = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(3),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(inner);
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), rows[0]);
        draw_button_row(frame, app, rows[1], &buttons, focused);
        return;
    }

    // The composer: list/fields above, buttons below.
    if let Some(Modal::Composer(composer)) = app.modals.last() {
        let buttons = composer.buttons();
        let focused = composer.focused_action();
        let lines = composer_lines(app, composer);
        let rows = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(3),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(inner);
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), rows[0]);
        draw_button_row(frame, app, rows[1], &buttons, focused);
        return;
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
                ratatui::layout::Constraint::Length(1),
            ])
            .split(inner);
        frame.render_widget(
            Paragraph::new(Span::styled(editor.help.clone(), app.styles.dimmed)),
            rows[0],
        );
        frame.render_widget(&editor.textarea, rows[1]);
        let error = editor.error.clone();
        if let Some(error) = error {
            frame.render_widget(
                Paragraph::new(Span::styled(error, app.styles.error)),
                rows[2],
            );
        }
        // Clickable equivalents of Ctrl-S / Esc, so accepting an edit is not
        // knowledge-gated on a chord. (Tab cannot reach them: it types a tab
        // character in here, which a code editor needs more.)
        draw_button_row(
            frame,
            app,
            rows[3],
            &[
                crate::tui::hit::ModalAction::EditorAccept,
                crate::tui::hit::ModalAction::EditorCancel,
            ],
            None,
        );
        return;
    }

    // The confirm dialog gets real Yes/No buttons (clickable; y/n/Enter/Esc
    // still work and stay the fast path).
    if let Some(Modal::Confirm { .. }) = app.modals.last() {
        let rows = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(2),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(inner);
        frame.render_widget(
            Paragraph::new(body_lines).wrap(Wrap { trim: false }),
            rows[0],
        );
        draw_button_row(
            frame,
            app,
            rows[1],
            &[
                crate::tui::hit::ModalAction::ConfirmYes,
                crate::tui::hit::ModalAction::ConfirmNo,
            ],
            None,
        );
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

fn form_lines<'a>(app: &DashboardApp, form: &crate::tui::modal::form::FormModel) -> Vec<Line<'a>> {
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
                format!(
                    "{:<22}",
                    format!("{}{}", field.label, if field.required { " *" } else { "" })
                ),
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

/// The handler editor's kind control: one segment per way of answering, the
/// selected one inverted, every segment clickable.
fn draw_kind_segments(
    frame: &mut ratatui::Frame,
    app: &mut DashboardApp,
    area: ratatui::layout::Rect,
    kind: crate::tui::modal::routing::HandlerKind,
    control_focused: bool,
) {
    use crate::tui::modal::routing::HandlerKind;

    let label = "respond with  ";
    let mut spans: Vec<Span> = vec![Span::styled(
        label,
        if control_focused {
            app.styles.accent
        } else {
            app.styles.normal
        },
    )];
    let mut x = area.x + label.chars().count() as u16;
    for candidate in HandlerKind::ALL {
        let text = format!(" {} ", candidate.label());
        let width = text.chars().count() as u16;
        if x + width > area.x + area.width {
            break;
        }
        let style = if candidate == kind {
            app.styles.selected
        } else {
            app.styles.button
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
        app.hits.push(
            ratatui::layout::Rect {
                x,
                y: area.y,
                width,
                height: 1,
            },
            HitTarget::ModalActionButton(crate::tui::hit::ModalAction::DraftKind(candidate)),
        );
        x += width + 1;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The draft editor's body: the pattern chooser, then the selected kind's own
/// fields. Every row here is a Tab stop; the `▸` marker tracks focus.
fn draft_body_lines<'a>(
    app: &DashboardApp,
    model: &crate::tui::modal::routing::RoutingModel,
    draft: &crate::tui::modal::routing::HandlerDraft,
) -> Vec<Line<'a>> {
    use crate::tui::modal::routing::{DraftFocus, HandlerDraft, HandlerKind};

    let mut lines = Vec::new();
    let marker = |focused: bool| Span::styled(if focused { "▸ " } else { "  " }, app.styles.accent);
    let label = |text: &str, focused: bool| {
        Span::styled(
            format!("{text:<18}"),
            if focused {
                app.styles.accent
            } else {
                app.styles.normal
            },
        )
    };

    // --- when ---
    let pattern_focused = draft.focus == DraftFocus::Pattern;
    let pattern_display = if draft.editing.is_some() && pattern_focused {
        format!("{}_", draft.editing.as_deref().unwrap_or(""))
    } else if draft.pattern == "*" {
        "* (every event)".to_string()
    } else {
        draft.pattern.clone()
    };
    lines.push(Line::from(vec![
        marker(pattern_focused),
        label("when event", pattern_focused),
        Span::styled(pattern_display, app.styles.info),
    ]));
    if pattern_focused && draft.editing.is_none() {
        // The chooser: ←/→ walks these; Enter types a pattern by hand.
        let choices = HandlerDraft::pattern_choices(&model.event_ids);
        for choice in choices.iter().take(12) {
            let selected = *choice == draft.pattern;
            let description = if choice == "*" {
                "matches every event".to_string()
            } else {
                model
                    .event_ids
                    .iter()
                    .find(|(id, _)| id == choice)
                    .map(|(_, d)| crate::utils::truncate_for_log(d, 56))
                    .unwrap_or_default()
            };
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(
                    format!("{choice:<28}"),
                    if selected {
                        app.styles.selected
                    } else {
                        app.styles.dimmed
                    },
                ),
                Span::styled(format!(" {description}"), app.styles.dimmed),
            ]));
        }
    }
    lines.push(Line::from(""));

    // --- the kind's own fields ---
    match draft.kind {
        HandlerKind::Static => {
            let focused = draft.focus == DraftFocus::Actions;
            lines.push(Line::from(vec![
                marker(focused),
                label("response actions", focused),
                Span::styled(
                    if draft.actions.is_empty() {
                        "(Enter opens the JSON editor, prefilled with a working example)"
                            .to_string()
                    } else {
                        format!("{} action(s) — Enter edits, d deletes", draft.actions.len())
                    },
                    if draft.actions.is_empty() {
                        app.styles.dimmed
                    } else {
                        app.styles.info
                    },
                ),
            ]));
            for (index, action) in draft.actions.iter().enumerate() {
                let name = action
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("(no type)");
                let style = if focused && index == draft.selected_action {
                    app.styles.selected
                } else {
                    app.styles.info
                };
                lines.push(Line::from(Span::styled(
                    format!(
                        "      {name}  {}",
                        crate::utils::truncate_for_log(&action.to_string(), 70)
                    ),
                    style,
                )));
            }
        }
        HandlerKind::Script => {
            let language_focused = draft.focus == DraftFocus::Language;
            lines.push(Line::from(vec![
                marker(language_focused),
                label("language", language_focused),
                Span::styled(format!("◂ {} ▸", draft.language), app.styles.info),
            ]));
            let code_focused = draft.focus == DraftFocus::Code;
            lines.push(Line::from(vec![
                marker(code_focused),
                label("script code", code_focused),
                Span::styled(
                    if draft.code.is_empty() {
                        "(Enter opens the editor)".to_string()
                    } else {
                        format!("{} line(s) — Enter edits", draft.code.lines().count())
                    },
                    if draft.code.is_empty() {
                        app.styles.dimmed
                    } else {
                        app.styles.info
                    },
                ),
            ]));
            for line in draft.code.lines().take(6) {
                lines.push(Line::from(Span::styled(
                    format!("      {line}"),
                    app.styles.dimmed,
                )));
            }
            let resident_focused = draft.focus == DraftFocus::Resident;
            lines.push(Line::from(vec![
                marker(resident_focused),
                label("resident", resident_focused),
                Span::styled(
                    if draft.resident {
                        "[x] one process stays alive and keeps state between events"
                    } else {
                        "[ ] a fresh interpreter per event (stateless)"
                    },
                    app.styles.info,
                ),
            ]));
        }
        HandlerKind::Llm => {
            let focused = draft.focus == DraftFocus::Instruction;
            lines.push(Line::from(vec![
                marker(focused),
                label("instruction", focused),
                Span::styled(
                    if draft.instruction.is_empty() {
                        "(Enter opens the editor)".to_string()
                    } else {
                        crate::utils::truncate_for_log(&draft.instruction, 70)
                    },
                    if draft.instruction.is_empty() {
                        app.styles.dimmed
                    } else {
                        app.styles.info
                    },
                ),
            ]));
        }
        HandlerKind::Manual => {
            let focused = draft.focus == DraftFocus::Timeout;
            let timeout_display = if draft.editing.is_some() && focused {
                format!("{}_", draft.editing.as_deref().unwrap_or(""))
            } else {
                format!("{} seconds", draft.timeout_secs)
            };
            lines.push(Line::from(vec![
                marker(focused),
                label("wait for me", focused),
                Span::styled(timeout_display, app.styles.info),
            ]));
            lines.push(Line::from(Span::styled(
                "      Each matched event appears under the instance as “⚠ waiting for YOUR \
                 answer”. No answer in time fails closed — the peer gets an error, never an \
                 invented success.",
                app.styles.dimmed,
            )));
        }
    }
    if let Some(error) = &draft.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("✗ {error}"),
            app.styles.error,
        )));
    }
    lines
}

/// A pending intercept: what arrived and the answer composed so far.
fn intercept_lines<'a>(
    app: &DashboardApp,
    model: &crate::tui::modal::intercept::InterceptModel,
) -> Vec<Line<'a>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("event      ", app.styles.dimmed),
            Span::styled(model.event_type.clone(), app.styles.accent),
        ]),
        Line::from(vec![
            Span::styled("what       ", app.styles.dimmed),
            Span::styled(model.description.clone(), app.styles.normal),
        ]),
    ];
    if let Some(data) = &model.event_data {
        lines.push(Line::from(Span::styled("payload", app.styles.dimmed)));
        let pretty = serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string());
        for line in pretty.lines().take(20) {
            lines.push(Line::from(Span::styled(
                format!("  {line}"),
                app.styles.info,
            )));
        }
        let hidden = pretty.lines().count().saturating_sub(20);
        if hidden > 0 {
            lines.push(Line::from(Span::styled(
                format!("  … {hidden} more line(s)"),
                app.styles.dimmed,
            )));
        }
    }
    lines.push(Line::from(""));
    if model.actions.is_empty() {
        lines.push(Line::from(Span::styled(
            "your answer   (empty — Send response proceeds without saying anything; \
             Compose actions… opens the editor with a working example)",
            app.styles.dimmed,
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!(
                "your answer   {} action(s): {}",
                model.actions.len(),
                model.action_names().join(", ")
            ),
            app.styles.success,
        )));
    }
    lines.push(Line::from(Span::styled(
        "The connection is waiting on you. Fail closed refuses it cleanly; Esc keeps it \
         waiting.",
        app.styles.dimmed,
    )));
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
                    lines.push(Line::from(Span::styled(line.to_string(), app.styles.info)));
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

fn band_detail_lines<'a>(app: &DashboardApp, key: crate::tui::app::UiKey) -> Vec<Line<'a>> {
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
                        row.local_addr
                            .clone()
                            .unwrap_or_else(|| format!(":{}", row.port))
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
