//! Keybinding reference. Doubles as the discovery surface for everything the
//! dashboard can do, which is why it enumerates the old slash commands too.

/// Lines of the help modal: `(heading, key, description)`; a `None` key marks
/// a section heading.
pub fn help_lines() -> Vec<(Option<&'static str>, &'static str)> {
    vec![
        (None, "Navigation — the rail is a tree"),
        (Some("Tab / Shift-Tab"), "move between chat, servers, clients"),
        (Some("↑ / ↓"), "walk the tree (crosses into the next instance)"),
        (Some("→"), "expand a group, or step into it"),
        (Some("←"), "collapse a group, or step out to its parent"),
        (Some("Enter / Space"), "toggle the row; on '… N more' show them all"),
        (Some("Enter on a request"), "expand its full request/response inline"),
        (Some("Esc"), "leave the tree, back to chat"),
        (Some("Space on an instance"), "maximize / restore that band"),
        (Some("PageUp / PageDown"), "scroll chat history"),
        (None, "Instances"),
        (Some("a"), "add: new server / client (protocol picker)"),
        (Some("e"), "edit the selected instance's config"),
        (Some("r"), "edit routing (LLM / script / static handlers)"),
        (Some("x"), "stop the selected instance (confirmed)"),
        (Some("c"), "on a server: connect a client of the same protocol"),
        (Some("n"), "on a client: compose and send a request"),
        (Some("d"), "protocol docs for the selected instance"),
        (None, "Global toggles"),
        (Some("Ctrl-L"), "cycle log level (filters chat, retroactively)"),
        (Some("Ctrl-W"), "cycle web search: on / ask / off"),
        (Some("Ctrl-H"), "cycle handler mode: any / script / static / llm"),
        (Some("Ctrl-E"), "cycle scripting mode"),
        (Some("Ctrl-T"), "toggle mouse capture (for native text selection)"),
        (Some("F1"), "this help"),
        (Some("Ctrl-C"), "quit"),
        (None, "Chat"),
        (Some("Enter"), "send to the LLM (or run a slash command)"),
        (Some("Alt-Enter / Ctrl-N"), "newline"),
        (Some("↑ / ↓"), "command history (at first/last line)"),
        (None, "Slash commands still work in chat"),
        (Some("/status /manage"), "superseded by the rail on the right"),
        (Some("/model /backend"), "also on the status bar (click it)"),
        (Some("/log /web /handler"), "also Ctrl-L / Ctrl-W / Ctrl-H"),
        (Some("/docs /env /usage"), "also d on a band, and F2"),
        (Some("/save /load"), "persist and restore instances"),
        (Some("/stop [id]"), "also x on a band"),
        (Some("/quit"), "also Ctrl-C"),
    ]
}
