use std::{
    fs::File,
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::{
    prompt_menu::menu_label, state::MenuItem, state::PromptState, validation,
};
use crate::util::parse_bool;

/// Source of key/input events so tests can drive the TUI without a real tty.
trait EventSource {
    fn next(&mut self, timeout: Duration) -> Result<Option<Event>>;
    fn is_scripted(&self) -> bool {
        false
    }
}

struct CrosstermEventSource;

impl EventSource for CrosstermEventSource {
    fn next(&mut self, timeout: Duration) -> Result<Option<Event>> {
        if event::poll(timeout)? {
            Ok(Some(event::read()?))
        } else {
            Ok(None)
        }
    }
}

/// Scripted event source driven by a simple line-oriented DSL:
///   down|up|left|right|enter|space|a|s|q|]|[|ctrl-s|type:<text>
/// Lines beginning with # are ignored. Blank lines are skipped.
/// When events are exhausted, we fail fast to avoid hangs.
struct ScriptEventSource {
    events: Vec<Event>,
    cursor: usize,
    exhausted_at: Option<Instant>,
    trace: Option<File>,
}

impl ScriptEventSource {
    fn from_path(path: PathBuf, trace_path: Option<PathBuf>) -> Result<Self> {
        let contents = std::fs::read_to_string(&path)
            .context("read scripted TUI input")?;
        let mut events = Vec::new();
        for (idx, raw) in contents.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut push_key = |code: KeyCode, modifiers: KeyModifiers| {
                events.push(Event::Key(KeyEvent {
                    code,
                    modifiers,
                    kind: event::KeyEventKind::Press,
                    state: event::KeyEventState::NONE,
                }));
            };

            match line {
                "down" | "j" => push_key(KeyCode::Down, KeyModifiers::NONE),
                "up" | "k" => push_key(KeyCode::Up, KeyModifiers::NONE),
                "left" => push_key(KeyCode::Left, KeyModifiers::NONE),
                "right" => push_key(KeyCode::Right, KeyModifiers::NONE),
                "enter" => push_key(KeyCode::Enter, KeyModifiers::NONE),
                "space" => push_key(KeyCode::Char(' '), KeyModifiers::NONE),
                "a" => push_key(KeyCode::Char('a'), KeyModifiers::NONE),
                "s" => push_key(KeyCode::Char('s'), KeyModifiers::NONE),
                "q" | "quit" => {
                    push_key(KeyCode::Char('q'), KeyModifiers::NONE)
                }
                "]" => push_key(KeyCode::Char(']'), KeyModifiers::NONE),
                "[" => push_key(KeyCode::Char('['), KeyModifiers::NONE),
                "ctrl-s" => push_key(KeyCode::Char('s'), KeyModifiers::CONTROL),
                _ => {
                    if let Some(rest) = line.strip_prefix("type:") {
                        for ch in rest.chars() {
                            push_key(KeyCode::Char(ch), KeyModifiers::NONE);
                        }
                    } else {
                        return Err(anyhow!(
                            "unrecognized TUI script token at line {}: {}",
                            idx + 1,
                            line
                        ));
                    }
                }
            }
        }

        let trace = trace_path
            .map(|p| File::create(p).context("create tui trace file"))
            .transpose()?;

        Ok(Self {
            events,
            cursor: 0,
            exhausted_at: None,
            trace,
        })
    }
}

impl EventSource for ScriptEventSource {
    fn next(&mut self, _timeout: Duration) -> Result<Option<Event>> {
        if self.cursor >= self.events.len() {
            // Allow a short grace period before failing to avoid tight loop.
            match self.exhausted_at {
                Some(ea) => {
                    if ea.elapsed() > Duration::from_secs(1) {
                        return Err(anyhow!(
                            "scripted TUI input exhausted before menu finished"
                        ));
                    }
                }
                None => self.exhausted_at = Some(Instant::now()),
            }
            std::thread::sleep(Duration::from_millis(25));
            return Ok(None);
        }

        let ev = self.events[self.cursor].clone();
        self.cursor += 1;

        if let Some(trace) = self.trace.as_mut() {
            let _ = writeln!(trace, "{:?}", ev);
        }

        Ok(Some(ev))
    }

    fn is_scripted(&self) -> bool {
        true
    }
}

enum Mode {
    Navigate,
    Editing(MenuItem),
}

enum MessageKind {
    Info,
    Success,
    Error,
}

struct StatusMessage {
    kind: MessageKind,
    text: String,
}

struct AppState {
    items: Vec<MenuItem>,
    selected: usize,
    show_advanced: bool,
    mode: Mode,
    input: String,
    message: Option<StatusMessage>,
    dirty: bool,
    pending_quit: bool,
}

impl AppState {
    fn new(show_advanced: bool) -> Self {
        let items = build_items(show_advanced);
        Self {
            items,
            selected: 0,
            show_advanced,
            mode: Mode::Navigate,
            input: String::new(),
            message: None,
            dirty: false,
            pending_quit: false,
        }
    }

    fn rebuild_items(&mut self) {
        self.items = build_items(self.show_advanced);
        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
    }

    fn jump_next_category(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let current_cat = item_category(self.items[self.selected]);
        let mut idx = self.selected + 1;
        // Skip the rest of the current category.
        while idx < self.items.len()
            && item_category(self.items[idx]) == current_cat
        {
            idx += 1;
        }
        if idx >= self.items.len() {
            return;
        }
        // Rewind to the first item in the next category block.
        let next_cat = item_category(self.items[idx]);
        while idx > 0 && item_category(self.items[idx - 1]) == next_cat {
            idx -= 1;
        }
        self.selected = idx;
    }

    fn jump_prev_category(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let current_cat = item_category(self.items[self.selected]);
        if self.selected == 0 {
            return;
        }
        // Move to the first item of the current category.
        let mut idx = self.selected;
        while idx > 0 && item_category(self.items[idx - 1]) == current_cat {
            idx -= 1;
        }
        if idx == 0 {
            return; // no previous category
        }
        // Step to the last item of the previous category.
        idx -= 1;
        let prev_cat = item_category(self.items[idx]);
        // Rewind to the first item of the previous category block.
        while idx > 0 && item_category(self.items[idx - 1]) == prev_cat {
            idx -= 1;
        }
        self.selected = idx;
    }

    fn set_message(&mut self, kind: MessageKind, text: impl Into<String>) {
        self.message = Some(StatusMessage {
            kind,
            text: text.into(),
        });
    }

    fn clear_message(&mut self) {
        self.message = None;
    }
}

pub(super) fn run_tui_menu(
    state: &mut PromptState,
    advanced_default: bool,
) -> Result<()> {
    let mut source = event_source_from_env()?;
    let scripted = source.is_scripted();

    let mut stdout = io::stdout();
    if !scripted {
        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = AppState::new(advanced_default);
    let result = run_app(&mut terminal, state, &mut app, &mut *source);

    if !scripted {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut PromptState,
    app: &mut AppState,
    source: &mut dyn EventSource,
) -> Result<()> {
    loop {
        terminal.draw(|f| render(f, state, app))?;

        if let Some(ev) = source.next(Duration::from_millis(150))? {
            match ev {
                Event::Key(key) => {
                    if handle_key(key, state, app)? {
                        return Ok(());
                    }
                }
                Event::Resize(_, _) => {
                    // redrawn on next loop automatically
                }
                _ => {}
            }
        }
    }
}

fn event_source_from_env() -> Result<Box<dyn EventSource>> {
    if let Ok(path) = std::env::var("FERREXCTL_TUI_SCRIPT") {
        let trace = std::env::var("FERREXCTL_TUI_TRACE").ok();
        let src = ScriptEventSource::from_path(
            PathBuf::from(path),
            trace.map(PathBuf::from),
        )?;
        Ok(Box::new(src))
    } else {
        Ok(Box::new(CrosstermEventSource))
    }
}

fn handle_key(
    key: KeyEvent,
    state: &mut PromptState,
    app: &mut AppState,
) -> Result<bool> {
    match app.mode {
        Mode::Navigate => {
            // Unsaved-changes guard on q (non-control).
            if let KeyCode::Char('q') = key.code
                && !key.modifiers.contains(KeyModifiers::CONTROL)
            {
                if app.dirty && !app.pending_quit {
                    app.pending_quit = true;
                    app.set_message(
                        MessageKind::Error,
                        "Unsaved changes — press q again to exit without writing .env",
                    );
                    return Ok(false);
                }
                return Ok(true);
            }

            // Any non-quit key cancels pending quit confirmation.
            app.pending_quit = false;

            match key.code {
                KeyCode::Char('s') => {
                    // Explicit finish; caller will write .env using current state.
                    return Ok(true);
                }
                KeyCode::Char('a') => {
                    app.show_advanced = !app.show_advanced;
                    app.rebuild_items();
                    let mode = if app.show_advanced {
                        "all (basic + advanced)"
                    } else {
                        "basic only"
                    };
                    app.set_message(
                        MessageKind::Info,
                        format!("Showing {mode} fields"),
                    );
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !app.items.is_empty() {
                        app.selected = (app.selected + 1) % app.items.len();
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if !app.items.is_empty() {
                        if app.selected == 0 {
                            app.selected = app.items.len() - 1;
                        } else {
                            app.selected -= 1;
                        }
                    }
                }
                KeyCode::Char(']') => {
                    app.jump_next_category();
                }
                KeyCode::Char('[') => {
                    app.jump_prev_category();
                }
                KeyCode::Enter => {
                    if app.items.is_empty() {
                        return Ok(false);
                    }
                    let item = app.items[app.selected];
                    if item == MenuItem::Finish {
                        return Ok(true);
                    }

                    if toggle_if_bool(item, state) {
                        app.dirty = true;
                        app.set_message(MessageKind::Success, "Toggled value");
                        return Ok(false);
                    }

                    app.input = current_value(item, state);
                    app.mode = Mode::Editing(item);
                    app.clear_message();
                }
                KeyCode::Char(' ') => {
                    let item = app.items.get(app.selected).copied();
                    if let Some(item) = item
                        && toggle_if_bool(item, state)
                    {
                        app.dirty = true;
                        app.set_message(MessageKind::Success, "Toggled value");
                    }
                }
                _ => {}
            }
        }
        Mode::Editing(item) => {
            match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Navigate;
                    app.input.clear();
                    app.clear_message();
                }
                KeyCode::Enter => {
                    let input = app.input.trim();
                    match apply_input(item, input, state) {
                        Ok(()) => {
                            app.dirty = true;
                            app.mode = Mode::Navigate;
                            app.input.clear();
                            app.pending_quit = false;
                            app.set_message(MessageKind::Success, "Saved");
                        }
                        Err(err) => {
                            app.set_message(
                                MessageKind::Error,
                                err.to_string(),
                            );
                        }
                    }
                }
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        // ignore control chars in edit mode (Ctrl+C handled outside)
                    } else {
                        app.input.push(c);
                    }
                }
                KeyCode::Backspace => {
                    app.input.pop();
                }
                KeyCode::Delete => {
                    // simple delete equivalent to backspace for single-line input
                    app.input.pop();
                }
                _ => {}
            }
        }
    }

    Ok(false)
}

fn render(f: &mut Frame, state: &PromptState, app: &AppState) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)].as_ref())
        .split(f.size());

    // Top: list on the left, details + help stacked on the right.
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [Constraint::Percentage(50), Constraint::Percentage(50)].as_ref(),
        )
        .split(vertical[0]);
    let list_area = main[0];
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [Constraint::Percentage(66), Constraint::Percentage(34)].as_ref(),
        )
        .split(main[1]);
    let detail_area = right[0];
    let help_area = right[1];

    // Navigation list with category headers
    let mut list_items: Vec<ListItem> = Vec::new();
    let mut selected_row: Option<usize> = None;
    let mut last_category: Option<&str> = None;

    for (idx, item) in app.items.iter().enumerate() {
        let category = item_category(*item);
        if Some(category) != last_category {
            let header = ListItem::new(Line::from(vec![Span::styled(
                category.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]));
            list_items.push(header);
            last_category = Some(category);
        }

        let row_label = menu_label(state, *item);
        let mut style = if row_label.contains("(unset)") {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
        };

        // Highlight booleans and Finish with color to make state obvious.
        style = match item {
            MenuItem::Finish => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            MenuItem::DevMode => {
                if state.dev_mode {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                }
            }
            MenuItem::CorsAllowCredentials => {
                if state.cors_allow_credentials {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                }
            }
            MenuItem::EnforceHttps => {
                if state.enforce_https {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                }
            }
            MenuItem::TrustProxy => {
                if state.trust_proxy_headers {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                }
            }
            MenuItem::HstsIncludeSub => {
                if state.hsts_include_subdomains {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                }
            }
            MenuItem::HstsPreload => {
                if state.hsts_preload {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                }
            }
            MenuItem::DemoMode => {
                if state.demo_mode {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                }
            }
            _ => style,
        };

        let row_index = list_items.len();
        if idx == app.selected {
            selected_row = Some(row_index);
        }
        let text = format!("  {row_label}");
        list_items.push(ListItem::new(text).style(style));
    }

    let list = List::new(list_items)
        .block(
            Block::default()
                .title(format!(
                    "Fields [{}] — a:toggle advanced, Enter:edit/finish",
                    if app.show_advanced { "all" } else { "basic" }
                ))
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::new()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut list_state = ratatui::widgets::ListState::default();
    if !app.items.is_empty() {
        // Highlight the row corresponding to the selected field, skipping headers.
        list_state.select(selected_row.or(Some(0)));
    }
    f.render_stateful_widget(list, list_area, &mut list_state);

    // Detail panel with colored sections
    let detail_lines: Vec<Line> = if app.items.is_empty() {
        vec![Line::from(Span::raw("No fields"))]
    } else {
        let item = app.items[app.selected];
        let label = menu_label(state, item);
        let help = help_text(item);
        let type_hint = field_type_hint(item);
        let raw_current = current_value(item, state);

        let (current_display, current_style) = match item {
            MenuItem::DevMode => {
                if state.dev_mode {
                    ("true".to_string(), Style::default().fg(Color::Green))
                } else {
                    ("false".to_string(), Style::default().fg(Color::Red))
                }
            }
            MenuItem::CorsAllowCredentials => {
                if state.cors_allow_credentials {
                    ("true".to_string(), Style::default().fg(Color::Green))
                } else {
                    ("false".to_string(), Style::default().fg(Color::DarkGray))
                }
            }
            MenuItem::EnforceHttps => {
                if state.enforce_https {
                    ("true".to_string(), Style::default().fg(Color::Green))
                } else {
                    ("false".to_string(), Style::default().fg(Color::Red))
                }
            }
            MenuItem::TrustProxy => {
                if state.trust_proxy_headers {
                    ("true".to_string(), Style::default().fg(Color::Green))
                } else {
                    ("false".to_string(), Style::default().fg(Color::DarkGray))
                }
            }
            MenuItem::HstsIncludeSub => {
                if state.hsts_include_subdomains {
                    ("true".to_string(), Style::default().fg(Color::Green))
                } else {
                    ("false".to_string(), Style::default().fg(Color::DarkGray))
                }
            }
            MenuItem::HstsPreload => {
                if state.hsts_preload {
                    ("true".to_string(), Style::default().fg(Color::Green))
                } else {
                    ("false".to_string(), Style::default().fg(Color::DarkGray))
                }
            }
            MenuItem::DemoMode => {
                if state.demo_mode {
                    ("true".to_string(), Style::default().fg(Color::Green))
                } else {
                    ("false".to_string(), Style::default().fg(Color::DarkGray))
                }
            }
            _ => {
                if raw_current.trim().is_empty() {
                    (
                        "(unset)".to_string(),
                        Style::default().fg(Color::DarkGray),
                    )
                } else {
                    (raw_current.clone(), Style::default().fg(Color::White))
                }
            }
        };

        let lines = vec![
            Line::from(Span::styled(
                label,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::default(),
            Line::from(vec![
                Span::styled("Category: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    item_category(item),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                type_hint,
                Style::default().fg(Color::Gray),
            )),
            Line::default(),
            Line::from(Span::styled(help, Style::default().fg(Color::White))),
            Line::default(),
            Line::from(vec![
                Span::styled("Mode: ", Style::default().fg(Color::Gray)),
                Span::styled(app.mode_name(), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Current: ", Style::default().fg(Color::Gray)),
                Span::styled(current_display, current_style),
            ]),
        ];
        lines
    };

    let detail = Paragraph::new(detail_lines)
        .block(Block::default().borders(Borders::ALL).title("Details"))
        .wrap(Wrap { trim: true });
    f.render_widget(detail, detail_area);

    // Help / legend pane in bottom-right
    let help_lines = vec![
        Line::from(Span::styled(
            "Keys:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("↑/↓, j/k  Move cursor"),
        Line::from("Enter     Edit field / finish"),
        Line::from("Space     Toggle boolean"),
        Line::from("[ / ]     Prev/next category"),
        Line::from("a         Basic/advanced fields"),
        Line::from("s         Save and exit"),
        Line::from("q         Quit (guarded)"),
    ];
    let help_widget = Paragraph::new(help_lines)
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .wrap(Wrap { trim: true });
    f.render_widget(help_widget, help_area);

    // Bottom status/help
    let status_text = match &app.mode {
        Mode::Navigate => "↑/↓ navigate • Enter edit • space toggle bool • [ / ] jump category • a basic/advanced • s save • q quit".to_string(),
        Mode::Editing(_) => format!("Editing: type, Enter save, Esc cancel | {}", app.input),
    };
    let default_message = if matches!(app.mode, Mode::Editing(_)) {
        ""
    } else {
        "Press s or select Finish to write .env"
    };
    let (message_text, message_style) = if let Some(msg) = &app.message {
        let style = match msg.kind {
            MessageKind::Info => Style::default().fg(Color::Gray),
            MessageKind::Success => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            MessageKind::Error => {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            }
        };
        (msg.text.as_str(), style)
    } else {
        (default_message, Style::default().fg(Color::DarkGray))
    };
    let status_line =
        Line::from(Span::styled(status_text, Style::default().fg(Color::Gray)));
    let message_line =
        Line::from(Span::styled(message_text.to_string(), message_style));
    let bottom = Paragraph::new(vec![status_line, message_line])
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(bottom, vertical[1]);
}

fn build_items(advanced: bool) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::Finish,
        MenuItem::DevMode,
        MenuItem::ServerHost,
        MenuItem::ServerPort,
        MenuItem::ServerUrl,
        MenuItem::MediaRoot,
        MenuItem::TmdbApiKey,
    ];
    if advanced {
        items.extend([
            MenuItem::CorsOrigins,
            MenuItem::CorsAllowCredentials,
            MenuItem::EnforceHttps,
            MenuItem::TrustProxy,
            MenuItem::HstsMaxAge,
            MenuItem::HstsIncludeSub,
            MenuItem::HstsPreload,
            MenuItem::TlsMinVersion,
            MenuItem::TlsCipherSuites,
            MenuItem::RateLimitsPath,
            MenuItem::RateLimitsJson,
            MenuItem::ScannerPath,
            MenuItem::ScannerJson,
            MenuItem::FfmpegPath,
            MenuItem::FfprobePath,
            MenuItem::DemoMode,
            MenuItem::DemoOptions,
            MenuItem::DemoUsername,
            MenuItem::DemoPassword,
            MenuItem::DemoAllowDeviations,
            MenuItem::DemoDeviationRate,
            MenuItem::DemoMovieCount,
            MenuItem::DemoSeriesCount,
            MenuItem::DemoSkipMetadata,
            MenuItem::DemoZeroLength,
        ]);
    }
    items
}

fn item_category(item: MenuItem) -> &'static str {
    match item {
        MenuItem::Finish => "Overview",
        MenuItem::DevMode
        | MenuItem::ServerHost
        | MenuItem::ServerPort
        | MenuItem::ServerUrl
        | MenuItem::MediaRoot
        | MenuItem::TmdbApiKey => "Core server",
        MenuItem::CorsOrigins | MenuItem::CorsAllowCredentials => {
            "CORS & frontend"
        }
        MenuItem::EnforceHttps
        | MenuItem::TrustProxy
        | MenuItem::HstsMaxAge
        | MenuItem::HstsIncludeSub
        | MenuItem::HstsPreload
        | MenuItem::TlsMinVersion
        | MenuItem::TlsCipherSuites => "HTTPS & TLS",
        MenuItem::RateLimitsPath | MenuItem::RateLimitsJson => "Rate limiting",
        MenuItem::ScannerPath
        | MenuItem::ScannerJson
        | MenuItem::FfmpegPath
        | MenuItem::FfprobePath => "Scanning & transcoding",
        MenuItem::DemoMode
        | MenuItem::DemoOptions
        | MenuItem::DemoUsername
        | MenuItem::DemoPassword
        | MenuItem::DemoAllowDeviations
        | MenuItem::DemoDeviationRate
        | MenuItem::DemoMovieCount
        | MenuItem::DemoSeriesCount
        | MenuItem::DemoSkipMetadata
        | MenuItem::DemoZeroLength => "Demo content",
    }
}

fn field_type_hint(item: MenuItem) -> &'static str {
    match item {
        MenuItem::Finish => "Action: finish and write .env",
        MenuItem::DevMode
        | MenuItem::CorsAllowCredentials
        | MenuItem::EnforceHttps
        | MenuItem::TrustProxy
        | MenuItem::HstsIncludeSub
        | MenuItem::HstsPreload
        | MenuItem::DemoMode => "Type: boolean (true/false/1/0/yes/no)",
        MenuItem::ServerPort => "Type: integer port (0-65535)",
        MenuItem::HstsMaxAge => "Type: integer seconds (0 disables HSTS)",
        MenuItem::MediaRoot
        | MenuItem::RateLimitsPath
        | MenuItem::ScannerPath => "Type: filesystem path (blank allowed)",
        MenuItem::RateLimitsJson
        | MenuItem::ScannerJson
        | MenuItem::DemoOptions => "Type: JSON object (blank allowed)",
        MenuItem::DemoDeviationRate => "Type: decimal fraction 0-1 (e.g., 0.1)",
        MenuItem::DemoMovieCount | MenuItem::DemoSeriesCount => {
            "Type: integer count (blank keeps default)"
        }
        MenuItem::DemoAllowDeviations
        | MenuItem::DemoSkipMetadata
        | MenuItem::DemoZeroLength => "Type: string (true/false recommended)",
        MenuItem::ServerHost => "Type: host/IP (e.g., 0.0.0.0 or 127.0.0.1)",
        MenuItem::ServerUrl => {
            "Type: URL (scheme://host:port, e.g., https://example.com)"
        }
        MenuItem::TmdbApiKey => "Type: string (TMDB API key)",
        MenuItem::CorsOrigins => {
            "Type: comma-separated URLs (e.g., http://localhost:5173,https://app.example.com)"
        }
        MenuItem::TlsMinVersion => "Type: TLS version string (1.2 or 1.3)",
        MenuItem::TlsCipherSuites => {
            "Type: comma-separated cipher suites (blank for rustls defaults)"
        }
        MenuItem::FfmpegPath | MenuItem::FfprobePath => {
            "Type: binary name or path (must be executable in PATH)"
        }
        MenuItem::DemoUsername | MenuItem::DemoPassword => {
            "Type: string (optional demo credentials)"
        }
    }
}

fn toggle_if_bool(item: MenuItem, state: &mut PromptState) -> bool {
    match item {
        MenuItem::DevMode => state.dev_mode = !state.dev_mode,
        MenuItem::CorsAllowCredentials => {
            state.cors_allow_credentials = !state.cors_allow_credentials
        }
        MenuItem::EnforceHttps => state.enforce_https = !state.enforce_https,
        MenuItem::TrustProxy => {
            state.trust_proxy_headers = !state.trust_proxy_headers
        }
        MenuItem::HstsIncludeSub => {
            state.hsts_include_subdomains = !state.hsts_include_subdomains
        }
        MenuItem::HstsPreload => state.hsts_preload = !state.hsts_preload,
        MenuItem::DemoMode => state.demo_mode = !state.demo_mode,
        _ => return false,
    }
    true
}

fn current_value(item: MenuItem, state: &PromptState) -> String {
    match item {
        MenuItem::DevMode => state.dev_mode.to_string(),
        MenuItem::ServerHost => state.server_host.clone(),
        MenuItem::ServerPort => state.server_port.to_string(),
        MenuItem::ServerUrl => state.ferrex_server_url.clone(),
        MenuItem::MediaRoot => state
            .media_root
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        MenuItem::TmdbApiKey => state.tmdb_api_key.clone(),
        MenuItem::CorsOrigins => state.cors_allowed_origins.clone(),
        MenuItem::CorsAllowCredentials => {
            state.cors_allow_credentials.to_string()
        }
        MenuItem::EnforceHttps => state.enforce_https.to_string(),
        MenuItem::TrustProxy => state.trust_proxy_headers.to_string(),
        MenuItem::HstsMaxAge => state.hsts_max_age.to_string(),
        MenuItem::HstsIncludeSub => state.hsts_include_subdomains.to_string(),
        MenuItem::HstsPreload => state.hsts_preload.to_string(),
        MenuItem::TlsMinVersion => state.tls_min_version.clone(),
        MenuItem::TlsCipherSuites => state.tls_cipher_suites.clone(),
        MenuItem::RateLimitsPath => state.rate_limits_path.clone(),
        MenuItem::RateLimitsJson => state.rate_limits_json.clone(),
        MenuItem::ScannerPath => state.scanner_config_path.clone(),
        MenuItem::ScannerJson => state.scanner_config_json.clone(),
        MenuItem::FfmpegPath => state.ffmpeg_path.clone(),
        MenuItem::FfprobePath => state.ffprobe_path.clone(),
        MenuItem::DemoMode => state.demo_mode.to_string(),
        MenuItem::DemoOptions => state.demo_options.clone(),
        MenuItem::DemoUsername => state.demo_username.clone(),
        MenuItem::DemoPassword => state.demo_password.clone(),
        MenuItem::DemoAllowDeviations => state.demo_allow_deviations.clone(),
        MenuItem::DemoDeviationRate => state.demo_deviation_rate.clone(),
        MenuItem::DemoMovieCount => state.demo_movie_count.clone(),
        MenuItem::DemoSeriesCount => state.demo_series_count.clone(),
        MenuItem::DemoSkipMetadata => state.demo_skip_metadata.clone(),
        MenuItem::DemoZeroLength => state.demo_zero_length.clone(),
        MenuItem::Finish => "Finish".to_string(),
    }
}

fn apply_input(
    item: MenuItem,
    input: &str,
    state: &mut PromptState,
) -> Result<()> {
    match item {
        MenuItem::ServerHost => state.server_host = input.to_string(),
        MenuItem::ServerPort => {
            state.server_port = input
                .parse::<u16>()
                .map_err(|_| anyhow!("enter a valid port (0-65535)"))?;
        }
        MenuItem::ServerUrl => state.ferrex_server_url = input.to_string(),
        MenuItem::MediaRoot => {
            if input.trim().is_empty() {
                state.media_root = None;
            } else {
                validation::validate_media_root(input)
                    .map_err(|e| anyhow!("{}", e))?;
                state.media_root = Some(std::path::PathBuf::from(input));
            }
        }
        MenuItem::TmdbApiKey => {
            validation::validate_tmdb_api_key(input)
                .map_err(|e| anyhow!("{}", e))?;
            state.tmdb_api_key = input.to_string();
        }
        MenuItem::CorsOrigins => state.cors_allowed_origins = input.to_string(),
        MenuItem::CorsAllowCredentials => {
            state.cors_allow_credentials = parse_bool(input)
                .ok_or_else(|| anyhow!("enter true/false/1/0/yes/no"))?;
        }
        MenuItem::EnforceHttps => {
            state.enforce_https = parse_bool(input)
                .ok_or_else(|| anyhow!("enter true/false/1/0/yes/no"))?;
        }
        MenuItem::TrustProxy => {
            state.trust_proxy_headers = parse_bool(input)
                .ok_or_else(|| anyhow!("enter true/false/1/0/yes/no"))?;
        }
        MenuItem::HstsMaxAge => {
            state.hsts_max_age = input
                .parse::<u64>()
                .map_err(|_| anyhow!("enter seconds as an integer"))?;
        }
        MenuItem::HstsIncludeSub => {
            state.hsts_include_subdomains = parse_bool(input)
                .ok_or_else(|| anyhow!("enter true/false/1/0/yes/no"))?;
        }
        MenuItem::HstsPreload => {
            state.hsts_preload = parse_bool(input)
                .ok_or_else(|| anyhow!("enter true/false/1/0/yes/no"))?;
        }
        MenuItem::TlsMinVersion => state.tls_min_version = input.to_string(),
        MenuItem::TlsCipherSuites => {
            state.tls_cipher_suites = input.to_string()
        }
        MenuItem::RateLimitsPath => state.rate_limits_path = input.to_string(),
        MenuItem::RateLimitsJson => state.rate_limits_json = input.to_string(),
        MenuItem::ScannerPath => state.scanner_config_path = input.to_string(),
        MenuItem::ScannerJson => state.scanner_config_json = input.to_string(),
        MenuItem::FfmpegPath => state.ffmpeg_path = input.to_string(),
        MenuItem::FfprobePath => state.ffprobe_path = input.to_string(),
        MenuItem::DemoMode => {
            state.demo_mode = parse_bool(input)
                .ok_or_else(|| anyhow!("enter true/false/1/0/yes/no"))?;
        }
        MenuItem::DemoOptions => state.demo_options = input.to_string(),
        MenuItem::DemoUsername => state.demo_username = input.to_string(),
        MenuItem::DemoPassword => state.demo_password = input.to_string(),
        MenuItem::DemoAllowDeviations => {
            state.demo_allow_deviations = input.to_string()
        }
        MenuItem::DemoDeviationRate => {
            state.demo_deviation_rate = input.to_string()
        }
        MenuItem::DemoMovieCount => state.demo_movie_count = input.to_string(),
        MenuItem::DemoSeriesCount => {
            state.demo_series_count = input.to_string()
        }
        MenuItem::DemoSkipMetadata => {
            state.demo_skip_metadata = input.to_string()
        }
        MenuItem::DemoZeroLength => state.demo_zero_length = input.to_string(),
        MenuItem::Finish => {}
        MenuItem::DevMode => {
            state.dev_mode = parse_bool(input)
                .ok_or_else(|| anyhow!("enter true/false/1/0/yes/no"))?;
        }
    }
    Ok(())
}

fn help_text(item: MenuItem) -> &'static str {
    match item {
        MenuItem::Finish => "Write .env and exit",
        MenuItem::DevMode => {
            "Use development-friendly defaults and repository_ports."
        }
        MenuItem::ServerHost => {
            "Bind address for ferrex-server (0.0.0.0 in containers)."
        }
        MenuItem::ServerPort => "Port ferrex-server listens on.",
        MenuItem::ServerUrl => "Public URL clients reach (used in redirects).",
        MenuItem::MediaRoot => "Path to your media library (optional).",
        MenuItem::TmdbApiKey => {
            "TMDB API key; blank disables metadata fetches."
        }
        MenuItem::CorsOrigins => {
            "Comma-separated list of allowed frontend origins."
        }
        MenuItem::CorsAllowCredentials => {
            "Allow cookies/headers in CORS responses."
        }
        MenuItem::EnforceHttps => {
            "Redirect HTTP to HTTPS and enable HSTS if set."
        }
        MenuItem::TrustProxy => {
            "Honor X-Forwarded-Proto/X-Forwarded-For headers."
        }
        MenuItem::HstsMaxAge => "Seconds for HSTS max-age (0 to disable).",
        MenuItem::HstsIncludeSub => "Apply HSTS to subdomains.",
        MenuItem::HstsPreload => "Opt into browser preload list.",
        MenuItem::TlsMinVersion => "TLS minimum version (1.2 or 1.3).",
        MenuItem::TlsCipherSuites => {
            "Comma-separated cipher suites (blank = defaults)."
        }
        MenuItem::RateLimitsPath => {
            "Path to rate limiter config file (optional)."
        }
        MenuItem::RateLimitsJson => {
            "Inline JSON for rate limiter (overrides path)."
        }
        MenuItem::ScannerPath => "Path to scanner config file (optional).",
        MenuItem::ScannerJson => {
            "Inline JSON for scanner config (overrides path)."
        }
        MenuItem::FfmpegPath => "Path to ffmpeg binary.",
        MenuItem::FfprobePath => "Path to ffprobe binary.",
        MenuItem::DemoMode => "Enable demo content seeding.",
        MenuItem::DemoOptions => "JSON blob with demo-mode options.",
        MenuItem::DemoUsername => "Demo login username.",
        MenuItem::DemoPassword => "Demo login password.",
        MenuItem::DemoAllowDeviations => {
            "Allow imperfect demo layouts (true/false)."
        }
        MenuItem::DemoDeviationRate => "Fraction 0-1 for demo deviations.",
        MenuItem::DemoMovieCount => "Number of demo movies to seed.",
        MenuItem::DemoSeriesCount => "Number of demo series to seed.",
        MenuItem::DemoSkipMetadata => "Skip metadata fetch during demo ingest.",
        MenuItem::DemoZeroLength => {
            "Generate zero-length demo files (true/false)."
        }
    }
}

impl AppState {
    fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Navigate => "Navigate",
            Mode::Editing(_) => "Editing",
        }
    }
}
