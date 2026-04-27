//! Full-screen TUI for brain. `brain tui` opens this.
//!
//! Layout:
//!   ┌ 🧠 brain ─────────────────────────────────────────────┐
//!   │ ┌─ Recent ─────┐  ┌─ Note ────────────────────────┐  │
//!   │ │ • fastapi    │  │ 2m ago                         │ │
//!   │ │ • ssd build  │  │                                │ │
//!   │ │ • tokio       │  │ picked SSD for build artifacts │ │
//!   │ └───────────────┘  └────────────────────────────────┘ │
//!   │ [/] search  [n] note  [r] reload  [d] doctor  [q] quit│
//!   └────────────────────────────────────────────────────────┘
//!
//! Modes:
//!   Normal  — j/k navigate, g/G jump, / search, n note, r reload, d doctor, q quit
//!   Search  — type query; Enter commits, Esc cancels
//!   Compose — type note; Enter saves, Esc cancels
//!   Doctor  — shows brain.doctor() report; any key closes
//!
//! Design choices:
//!   - 120ms poll — cheap wake-up cadence keeps the UI responsive to
//!     resize events and typing without burning CPU.
//!   - LocalBrain handles are reopened per action. Cheap on SSD; avoids
//!     holding a git2 Repository across await/render boundaries.
//!   - No async. crossterm event loop is sync; local work (search,
//!     doctor, save) is called inline and usually <10ms. Network-backed
//!     push/pull can block and reports progress via the status flash.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use brain_app::LocalBrain;
use brain_types::{Event, EventPayload};
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};

/// Entry point. Sets up the alternate screen + raw mode, runs the loop,
/// then tears down even on setup failure or panic.
///
/// Codex R14 P1: the guard is armed BEFORE any step that can fail. If
/// `EnterAlternateScreen` or `Terminal::new` errors out, the guard's
/// Drop still fires and restores the terminal.
pub fn run(brain: LocalBrain) -> Result<()> {
    let (mut terminal, _guard) = setup_terminal().context("enter tui")?;
    App::new(brain).run(&mut terminal)
    // _guard drops here on both Ok and Err paths, restoring the terminal.
}

fn setup_terminal() -> io::Result<(Terminal<CrosstermBackend<Stdout>>, TerminalGuard)> {
    enable_raw_mode()?;
    let guard = TerminalGuard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let term = Terminal::new(CrosstermBackend::new(stdout))?;
    Ok((term, guard))
}

/// RAII teardown. Restores terminal on Drop — so scope exit,
/// early return, and panic unwind all trigger cleanup.
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Search,
    Compose,
    Doctor,
}

struct App {
    brain: LocalBrain,
    mode: Mode,
    /// Raw events loaded from the log (unfiltered).
    all_events: Vec<Event>,
    /// Filtered view. This is what the list renders from.
    events: Vec<Event>,
    list_state: ListState,
    search_input: String,
    compose_input: String,
    /// Current tool filter. None = all tools.
    tool_filter: Option<String>,
    /// Rotating list of tool ids we've seen — lets `t` cycle.
    tool_cycle: Vec<String>,
    /// Last saved / last action message shown in the status bar.
    flash: Option<(String, Instant)>,
    /// Cached doctor report while in Doctor mode.
    doctor_report: Option<String>,
    /// Set to true to quit the main loop after the current frame.
    quit: bool,
}

impl App {
    fn new(brain: LocalBrain) -> Self {
        let mut app = Self {
            brain,
            mode: Mode::Normal,
            all_events: Vec::new(),
            events: Vec::new(),
            list_state: ListState::default(),
            search_input: String::new(),
            compose_input: String::new(),
            tool_filter: None,
            tool_cycle: Vec::new(),
            flash: None,
            doctor_report: None,
            quit: false,
        };
        let _ = app.reload_log();
        app
    }

    /// Cycle through `None → tool[0] → tool[1] → ... → None` on each `t`.
    fn cycle_tool_filter(&mut self) {
        if self.tool_cycle.is_empty() {
            return;
        }
        self.tool_filter = match &self.tool_filter {
            None => Some(self.tool_cycle[0].clone()),
            Some(cur) => {
                let idx = self.tool_cycle.iter().position(|t| t == cur);
                match idx {
                    Some(i) if i + 1 < self.tool_cycle.len() => {
                        Some(self.tool_cycle[i + 1].clone())
                    }
                    _ => None,
                }
            }
        };
        self.apply_filter();
    }

    /// Rebuild `events` from `all_events` under the current tool_filter.
    /// Search mode also applies this filter to FTS hits.
    fn apply_filter(&mut self) {
        self.events = self.apply_tool_filter(self.all_events.clone());
        self.list_state.select(if self.events.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    fn apply_tool_filter(&self, events: Vec<Event>) -> Vec<Event> {
        match &self.tool_filter {
            None => events,
            Some(tool) => events.into_iter().filter(|e| &e.actor.id == tool).collect(),
        }
    }

    fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.render(f))?;
            self.tick()?;
        }
        Ok(())
    }

    fn tick(&mut self) -> Result<()> {
        if !event::poll(Duration::from_millis(120))? {
            return Ok(());
        }
        if let CtEvent::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                self.handle_key(key.code, key.modifiers);
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        // Global: Ctrl-C always exits cleanly.
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match self.mode {
            Mode::Normal => self.handle_normal(code),
            Mode::Search => self.handle_search(code),
            Mode::Compose => self.handle_compose(code, mods),
            Mode::Doctor => self.handle_doctor(code),
        }
    }

    fn handle_normal(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.select_delta(1),
            KeyCode::Char('k') | KeyCode::Up => self.select_delta(-1),
            KeyCode::Char('g') => self.list_state.select(Some(0)),
            KeyCode::Char('G') => {
                if !self.events.is_empty() {
                    self.list_state.select(Some(self.events.len() - 1));
                }
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Search;
                self.search_input.clear();
            }
            KeyCode::Char('n') => {
                self.mode = Mode::Compose;
                self.compose_input.clear();
            }
            KeyCode::Char('d') => {
                self.open_doctor();
            }
            KeyCode::Char('r') => {
                // Refresh.
                if let Err(e) = self.reload_log() {
                    self.flash_err(format!("reload failed: {e}"));
                }
            }
            KeyCode::Char('t') => {
                // Cycle tool filter: None → tool_0 → tool_1 → ... → None.
                self.cycle_tool_filter();
                let label = match &self.tool_filter {
                    None => "all tools".into(),
                    Some(t) => format!("tool: {t}"),
                };
                self.flash(label);
            }
            KeyCode::Char('p') => {
                // Pull from default remote.
                self.flash("pulling…".into());
                match self.brain.pull(None) {
                    Ok(_) => {
                        self.flash("pulled".into());
                        if let Err(e) = self.reload_log() {
                            self.flash_err(format!("reload after pull: {e}"));
                        }
                    }
                    Err(e) => self.flash_err(format!("pull: {e}")),
                }
            }
            KeyCode::Char('P') => {
                // Shift-P: push to default remote.
                self.flash("pushing…".into());
                match self.brain.push(None) {
                    Ok(_) => self.flash("pushed".into()),
                    Err(e) => self.flash_err(format!("push: {e}")),
                }
            }
            _ => {}
        }
    }

    fn handle_search(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.search_input.clear();
                self.mode = Mode::Normal;
                if let Err(e) = self.reload_log() {
                    self.flash_err(format!("reload after search: {e}"));
                }
            }
            KeyCode::Enter => {
                // Commit: keep the narrowed view, exit search mode.
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.search_input.pop();
                self.apply_search();
            }
            KeyCode::Char(c) => {
                self.search_input.push(c);
                self.apply_search();
            }
            _ => {}
        }
    }

    /// Live-narrow the list to FTS5 hits for the current query. Empty
    /// input shows the full recent log. Runs on every keystroke, so
    /// FTS5 parse errors are silenced inline (malformed query is the
    /// user's in-progress typing, not a system failure). Non-parse
    /// errors DO flash so a corrupt index doesn't look identical to
    /// "no matches."
    fn apply_search(&mut self) {
        let q = self.search_input.trim();
        if q.is_empty() {
            if let Err(e) = self.reload_log() {
                self.flash_err(format!("reload: {e}"));
            }
            return;
        }
        match self.brain.search(q, 50) {
            Ok(hits) => {
                self.events = self.apply_tool_filter(hits);
                self.list_state.select(if self.events.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            Err(e) => {
                // FTS5 parse errors are now swallowed one level down
                // (BrainIndex::search returns Ok(empty)), so anything
                // that reaches here is a real failure worth surfacing.
                self.events.clear();
                self.list_state.select(None);
                self.flash_err(format!("search: {e}"));
            }
        }
    }

    fn handle_compose(&mut self, code: KeyCode, mods: KeyModifiers) {
        match code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.compose_input.clear();
            }
            KeyCode::Enter if mods.contains(KeyModifiers::SHIFT) => {
                self.compose_input.push('\n');
            }
            KeyCode::Enter => {
                if self.compose_input.trim().is_empty() {
                    self.flash_err("note is empty".into());
                    return;
                }
                // Codex R14 P2: clone for the save, keep the buffer
                // intact. On error the user stays in Compose mode with
                // their text still there — fix and retry. Only clear
                // after a successful commit.
                let text = self.compose_input.clone();
                match self.brain.observe_summary(text) {
                    Ok(ev) => {
                        let msg = if ev.was_idempotent_replay {
                            "already saved".into()
                        } else {
                            "saved".into()
                        };
                        self.flash(msg);
                        self.compose_input.clear();
                        self.mode = Mode::Normal;
                        if let Err(e) = self.reload_log() {
                            self.flash_err(format!("reload after save: {e}"));
                        }
                    }
                    Err(e) => self.flash_err(format!("save: {e}")),
                }
            }
            KeyCode::Backspace => {
                self.compose_input.pop();
            }
            KeyCode::Char(c) => {
                self.compose_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_doctor(&mut self, _code: KeyCode) {
        // Any key closes.
        self.mode = Mode::Normal;
        self.doctor_report = None;
    }

    fn select_delta(&mut self, d: i32) {
        if self.events.is_empty() {
            self.list_state.select(None);
            return;
        }
        let len = self.events.len() as i32;
        let cur = self.list_state.selected().map(|s| s as i32).unwrap_or(0);
        let next = (cur + d).rem_euclid(len);
        self.list_state.select(Some(next as usize));
    }

    fn reload_log(&mut self) -> Result<()> {
        self.all_events = self.brain.log(100).context("reload log")?;
        // Collect unique actor ids for the tool-filter cycle. Sorted
        // so `t` cycles deterministically instead of hash-order.
        let mut seen = std::collections::BTreeSet::new();
        for e in &self.all_events {
            seen.insert(e.actor.id.clone());
        }
        self.tool_cycle = seen.into_iter().collect();
        self.apply_filter();
        Ok(())
    }

    fn open_doctor(&mut self) {
        match self.brain.doctor() {
            Ok(r) => {
                let mut out = String::new();
                out.push_str(&format!("brain @ {}\n", r.path.display()));
                out.push_str(&format!("schema: v{}\n", r.schema_version));
                out.push_str(&format!(
                    "events: {} in {} commits\n",
                    r.event_count, r.commit_count
                ));
                out.push_str(&format!(
                    "indexed: {} (lag {})\n",
                    r.indexed_event_count, r.index_lag
                ));
                if let Some(t) = r.oldest_event_at {
                    out.push_str(&format!("oldest: {}\n", t.format("%Y-%m-%d %H:%M")));
                }
                if let Some(t) = r.newest_event_at {
                    out.push_str(&format!("newest: {}\n", t.format("%Y-%m-%d %H:%M")));
                }
                if !r.warnings.is_empty() {
                    out.push_str("\nwarnings:\n");
                    for warning in &r.warnings {
                        out.push_str(&format!("  • {warning}\n"));
                    }
                }
                if r.issues.is_empty() {
                    out.push_str("\nno issues.");
                } else {
                    out.push_str("\nissues:\n");
                    for issue in &r.issues {
                        out.push_str(&format!("  • {issue}\n"));
                    }
                }
                self.doctor_report = Some(out);
                self.mode = Mode::Doctor;
            }
            Err(e) => self.flash_err(format!("doctor: {e}")),
        }
    }

    fn flash(&mut self, msg: String) {
        self.flash = Some((msg, Instant::now()));
    }

    fn flash_err(&mut self, msg: String) {
        // Same slot; render colors errors differently based on content.
        self.flash = Some((format!("error: {msg}"), Instant::now()));
    }

    // --- rendering -----------------------------------------------------
    //
    // Design principles (opentui-inspired):
    //   - No heavy box-drawing borders. Whitespace + single thin rules.
    //   - Truecolor muted palette. Dim rest, bright selected.
    //   - Half-block accents (▎ ▏) instead of colored bg blocks.
    //   - Search is live: every keystroke narrows, no Enter needed.
    //   - Empty states are centered + quiet, not alarming.

    fn render(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        // Reserve bottom row for status / search bar. Search mode
        // promotes the bottom row to a live input; Normal shows
        // keybinds + flash.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // header (icon + path + counts)
                Constraint::Min(3),    // main (list | detail)
                Constraint::Length(1), // status / search
            ])
            .split(area);

        self.render_header(f, chunks[0]);
        self.render_main(f, chunks[1]);
        self.render_status(f, chunks[2]);

        match self.mode {
            Mode::Compose => self.render_compose_card(f, area),
            Mode::Doctor => self.render_doctor_card(f, area),
            Mode::Search | Mode::Normal => {}
        }
    }

    fn render_header(&self, f: &mut ratatui::Frame, area: Rect) {
        // Line 1: ▏brain   <path>                        <n notes>
        // Line 2: (empty — vertical breathing room)
        let count = self.events.len();
        let total = self.all_events.len();
        let tool_count = self.tool_cycle.len();
        let count_label = match count {
            0 => "empty".to_string(),
            1 => "1 note".to_string(),
            n => format!("{n} notes"),
        };
        // Show active filters inline so the user knows they're in a
        // narrowed view: "3 notes · tool: claude-code · of 47"
        let filter_tag = if self.mode == Mode::Search && !self.search_input.is_empty() {
            "match · ".to_string()
        } else if self.tool_filter.is_some() {
            format!("{} · ", self.tool_filter.as_deref().unwrap_or(""))
        } else {
            String::new()
        };
        let of_tail = if count < total {
            format!(" · of {total}")
        } else {
            String::new()
        };

        let path = self.brain.path().display().to_string();
        let path_display = compact_path(&path, area.width as usize / 2);

        let left = Line::from(vec![
            Span::styled("▏", Style::new().fg(Pal::ACCENT)),
            Span::styled(
                " brain ",
                Style::new().fg(Pal::FG).add_modifier(Modifier::BOLD),
            ),
            Span::styled("·", Style::new().fg(Pal::DIM)),
            Span::raw(" "),
            Span::styled(path_display, Style::new().fg(Pal::DIM)),
            Span::raw("  "),
            Span::styled(
                format!(
                    "{tool_count} tool{}",
                    if tool_count == 1 { "" } else { "s" }
                ),
                Style::new().fg(Pal::DIMMER),
            ),
        ]);
        let right = Line::from(vec![Span::styled(
            format!("{filter_tag}{count_label}{of_tail}"),
            Style::new().fg(Pal::DIM),
        )]);

        // Pack left+right into one row using a two-column layout.
        let row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(right_width(&right))])
            .split(Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 1,
            });
        f.render_widget(Paragraph::new(left), row[0]);
        f.render_widget(
            Paragraph::new(right).alignment(ratatui::layout::Alignment::Right),
            row[1],
        );
        // Row 2 of header is a thin rule, full width, dim.
        if area.height > 1 {
            let rule = "─".repeat(area.width as usize);
            f.render_widget(
                Paragraph::new(Span::styled(rule, Style::new().fg(Pal::RULE))),
                Rect {
                    x: area.x,
                    y: area.y + 1,
                    width: area.width,
                    height: 1,
                },
            );
        }
    }

    fn render_main(&mut self, f: &mut ratatui::Frame, area: Rect) {
        // 38/62 split feels better than 50/50 — list is the anchor, detail
        // needs more room to breathe.
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(38),
                Constraint::Length(1), // divider
                Constraint::Min(1),    // detail
            ])
            .split(area);

        self.render_list(f, cols[0]);
        self.render_divider(f, cols[1]);
        self.render_detail(f, cols[2]);
    }

    fn render_divider(&self, f: &mut ratatui::Frame, area: Rect) {
        // Single vertical rule. More elegant than a full bordered box.
        for dy in 0..area.height {
            let y = area.y + dy;
            f.render_widget(
                Paragraph::new(Span::styled("│", Style::new().fg(Pal::RULE))),
                Rect {
                    x: area.x,
                    y,
                    width: 1,
                    height: 1,
                },
            );
        }
    }

    fn render_list(&mut self, f: &mut ratatui::Frame, area: Rect) {
        if self.events.is_empty() {
            let msg = if !self.search_input.is_empty() {
                "no matches"
            } else {
                "no notes yet"
            };
            let hint = if !self.search_input.is_empty() {
                "try a different query"
            } else {
                "press n to save one"
            };
            let body = vec![
                Line::raw(""),
                Line::raw(""),
                Line::from(Span::styled(msg, Style::new().fg(Pal::DIM))).centered(),
                Line::raw(""),
                Line::from(Span::styled(hint, Style::new().fg(Pal::DIMMER))).centered(),
            ];
            f.render_widget(Paragraph::new(body), area);
            return;
        }

        let selected = self.list_state.selected();
        // Build items with day-group section headers. Headers are
        // non-selectable (ratatui has no native concept, so we emit
        // them as regular items — selection skips them because the
        // selected-row index maps to positions in `self.events`,
        // while headers live at synthetic indexes. To keep the
        // mapping simple we render the list as Vec<ListItem> but
        // maintain a parallel `row_to_event` map for selection.
        //
        // Simpler pragmatic choice: headers are rendered inline
        // between rows, but the `ListState` selection still indexes
        // into `self.events`. We paint the header line ABOVE the
        // first row of each bucket and let ratatui's cursor land on
        // the row.
        let mut items: Vec<ListItem> = Vec::with_capacity(self.events.len() + 5);
        let mut last_bucket: Option<&str> = None;
        for (i, ev) in self.events.iter().enumerate() {
            let bucket = day_bucket(ev.time_recorded);
            if last_bucket != Some(bucket) {
                // Emit a section header before this row. It will NOT
                // be selectable because highlight_symbol + selection
                // land on the row below it. Note: this confuses
                // list_state.selected() → event index mapping when
                // headers are injected, so we use a plain non-
                // stateful render path for this list.
                items.push(ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        bucket,
                        Style::new().fg(Pal::DIMMER).add_modifier(Modifier::BOLD),
                    ),
                ])));
                last_bucket = Some(bucket);
            }

            let is_sel = Some(i) == selected;
            let when = short_when(ev.time_recorded);
            let summary = ev
                .payload
                .summary_line(area.width.saturating_sub(18) as usize);
            let type_g = type_glyph(&ev.event_type);
            let tool_g = tool_glyph(&ev.actor.id);
            let text_style = if is_sel {
                Style::new().fg(Pal::FG).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Pal::DIM)
            };
            let when_style = if is_sel {
                Style::new().fg(Pal::ACCENT)
            } else {
                Style::new().fg(Pal::DIMMER)
            };
            let glyph_style = Style::new().fg(type_color(&ev.event_type));
            let sel_prefix = if is_sel { "▎" } else { " " };
            let line = Line::from(vec![
                Span::styled(sel_prefix, Style::new().fg(Pal::ACCENT)),
                Span::raw(format!("{tool_g} ")),
                Span::styled(format!("{type_g} "), glyph_style),
                Span::styled(format!("{:>8}", when), when_style),
                Span::raw("  "),
                Span::styled(summary, text_style),
            ]);
            items.push(ListItem::new(line));
        }

        // No highlight_symbol — we paint our own ▎ inline because the
        // header rows would otherwise misalign with the selection
        // cursor. Selection feedback is the bold + accent color + ▎.
        let list = List::new(items);
        f.render_widget(list, area);
    }

    fn render_detail(&self, f: &mut ratatui::Frame, area: Rect) {
        let selected = self.list_state.selected().and_then(|i| self.events.get(i));
        let padded = Rect {
            x: area.x + 1,
            y: area.y,
            width: area.width.saturating_sub(1),
            height: area.height,
        };

        let body: Vec<Line> = match selected {
            Some(ev) => {
                let mut out = Vec::new();
                let type_str = format!("{:?}", ev.event_type).to_lowercase();
                // Top: TYPE · time ago
                out.push(Line::from(vec![
                    Span::styled(
                        type_glyph(&ev.event_type),
                        Style::new().fg(type_color(&ev.event_type)),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        type_str.to_uppercase(),
                        Style::new()
                            .fg(type_color(&ev.event_type))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("   "),
                    Span::styled(short_when(ev.time_recorded), Style::new().fg(Pal::DIM)),
                ]));
                out.push(Line::raw(""));
                // Key/value metadata
                let kv = [
                    (
                        "saved",
                        ev.time_recorded.format("%Y-%m-%d %H:%M").to_string(),
                    ),
                    ("id   ", ev.event_id.to_string()),
                ];
                for (k, v) in kv {
                    out.push(Line::from(vec![
                        Span::styled(k, Style::new().fg(Pal::DIM)),
                        Span::raw("  "),
                        Span::styled(v, Style::new().fg(Pal::FG)),
                    ]));
                }
                out.push(Line::raw(""));
                // Thin separator
                out.push(Line::from(Span::styled(
                    "─".repeat(padded.width.saturating_sub(1) as usize),
                    Style::new().fg(Pal::RULE),
                )));
                out.push(Line::raw(""));
                for line in payload_body(&ev.payload).lines() {
                    out.push(Line::raw(line.to_string()));
                }
                out
            }
            None => vec![
                Line::raw(""),
                Line::raw(""),
                Line::from(Span::styled(
                    "select a note on the left",
                    Style::new().fg(Pal::DIMMER),
                ))
                .centered(),
            ],
        };

        let p = Paragraph::new(body).wrap(Wrap { trim: false });
        f.render_widget(p, padded);
    }

    fn render_status(&self, f: &mut ratatui::Frame, area: Rect) {
        if self.mode == Mode::Search {
            // Live search bar replaces status row.
            let line = Line::from(vec![
                Span::styled(
                    "/ ",
                    Style::new().fg(Pal::ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(&self.search_input, Style::new().fg(Pal::FG)),
                Span::styled("▏", Style::new().fg(Pal::ACCENT)),
                Span::raw("   "),
                Span::styled(
                    if self.events.is_empty() && !self.search_input.is_empty() {
                        "no matches · esc to clear"
                    } else {
                        "live · enter keeps view · esc clears"
                    },
                    Style::new().fg(Pal::DIMMER),
                ),
            ]);
            f.render_widget(Paragraph::new(line), area);
            return;
        }

        // Normal status: keybinds left, flash right.
        let keys = [
            ("/", "search"),
            ("n", "note"),
            ("t", "tool"),
            ("r", "reload"),
            ("p", "pull"),
            ("P", "push"),
            ("d", "doctor"),
            ("q", "quit"),
        ];
        let mut spans: Vec<Span> = Vec::new();
        for (i, (k, label)) in keys.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("  ", Style::new().fg(Pal::DIMMER)));
            }
            spans.push(Span::styled(
                *k,
                Style::new().fg(Pal::ACCENT).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(format!(" {label}"), Style::new().fg(Pal::DIM)));
        }
        let left_line = Line::from(spans);

        let flash_span: Option<Span> = self
            .flash
            .as_ref()
            .filter(|(_, t)| t.elapsed() < Duration::from_secs(5))
            .map(|(msg, _)| {
                let style = if msg.starts_with("error:") {
                    Style::new().fg(Pal::WARN)
                } else {
                    Style::new().fg(Pal::OK)
                };
                Span::styled(msg.clone(), style)
            });

        match flash_span {
            Some(fl) => {
                let row = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Min(1),
                        Constraint::Length(fl.content.chars().count() as u16 + 1),
                    ])
                    .split(area);
                f.render_widget(Paragraph::new(left_line), row[0]);
                f.render_widget(
                    Paragraph::new(Line::from(fl)).alignment(ratatui::layout::Alignment::Right),
                    row[1],
                );
            }
            None => {
                f.render_widget(Paragraph::new(left_line), area);
            }
        }
    }

    fn render_compose_card(&self, f: &mut ratatui::Frame, area: Rect) {
        // Centered card. No box — top + bottom rule, accent-colored
        // label on top-left, hint bottom-right.
        let card = centered_rect(area, 64, 10);
        f.render_widget(Clear, card);

        // Top rule with inline label
        let top_label = " new note ";
        let top_rule_left = "─".to_string();
        let filler = "─".repeat(
            (card.width as usize)
                .saturating_sub(top_rule_left.chars().count() + top_label.chars().count()),
        );
        let top = Line::from(vec![
            Span::styled(top_rule_left, Style::new().fg(Pal::RULE)),
            Span::styled(
                top_label,
                Style::new().fg(Pal::ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(filler, Style::new().fg(Pal::RULE)),
        ]);
        f.render_widget(
            Paragraph::new(top),
            Rect {
                x: card.x,
                y: card.y,
                width: card.width,
                height: 1,
            },
        );

        // Body
        let body_area = Rect {
            x: card.x + 1,
            y: card.y + 2,
            width: card.width.saturating_sub(2),
            height: card.height.saturating_sub(3),
        };
        let mut lines: Vec<Line> = self
            .compose_input
            .lines()
            .map(|l| Line::raw(l.to_string()))
            .collect();
        if self.compose_input.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("▏", Style::new().fg(Pal::ACCENT)),
                Span::styled(
                    "  type your note. shift+enter = newline.",
                    Style::new().fg(Pal::DIMMER),
                ),
            ]));
        } else if self.compose_input.ends_with('\n') {
            lines.push(Line::from(Span::styled("▏", Style::new().fg(Pal::ACCENT))));
        } else if let Some(last) = lines.last_mut() {
            last.spans
                .push(Span::styled("▏", Style::new().fg(Pal::ACCENT)));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body_area);

        // Bottom rule + hint
        let bottom_y = card.y + card.height.saturating_sub(1);
        let hint = "enter save · esc cancel";
        let filler = "─".repeat((card.width as usize).saturating_sub(hint.chars().count() + 3));
        let bottom = Line::from(vec![
            Span::styled(filler, Style::new().fg(Pal::RULE)),
            Span::styled(" ", Style::new()),
            Span::styled(hint, Style::new().fg(Pal::DIMMER)),
            Span::styled("──", Style::new().fg(Pal::RULE)),
        ]);
        f.render_widget(
            Paragraph::new(bottom),
            Rect {
                x: card.x,
                y: bottom_y,
                width: card.width,
                height: 1,
            },
        );
    }

    fn render_doctor_card(&self, f: &mut ratatui::Frame, area: Rect) {
        let card = centered_rect(area, 64, 16);
        f.render_widget(Clear, card);
        let top_label = " doctor ";
        let filler =
            "─".repeat((card.width as usize).saturating_sub(1 + top_label.chars().count()));
        let top = Line::from(vec![
            Span::styled("─", Style::new().fg(Pal::RULE)),
            Span::styled(
                top_label,
                Style::new().fg(Pal::ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(filler, Style::new().fg(Pal::RULE)),
        ]);
        f.render_widget(
            Paragraph::new(top),
            Rect {
                x: card.x,
                y: card.y,
                width: card.width,
                height: 1,
            },
        );
        let body_area = Rect {
            x: card.x + 2,
            y: card.y + 2,
            width: card.width.saturating_sub(4),
            height: card.height.saturating_sub(3),
        };
        let body = self.doctor_report.as_deref().unwrap_or("loading…");
        f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), body_area);
        let bottom_y = card.y + card.height.saturating_sub(1);
        let hint = "any key closes";
        let filler = "─".repeat((card.width as usize).saturating_sub(hint.chars().count() + 3));
        let bottom = Line::from(vec![
            Span::styled(filler, Style::new().fg(Pal::RULE)),
            Span::styled(" ", Style::new()),
            Span::styled(hint, Style::new().fg(Pal::DIMMER)),
            Span::styled("──", Style::new().fg(Pal::RULE)),
        ]);
        f.render_widget(
            Paragraph::new(bottom),
            Rect {
                x: card.x,
                y: bottom_y,
                width: card.width,
                height: 1,
            },
        );
    }
}

/// Truecolor palette. Picked to feel like opentui: muted, modern,
/// works on both light and dark terminal themes because we never
/// set a background color on the main content.
struct Pal;
impl Pal {
    /// Default foreground — we don't set this, letting the terminal
    /// theme own it. Kept as a named constant so the code reads
    /// symmetrically with DIM/ACCENT.
    const FG: Color = Color::Reset;
    /// Metadata, inactive list rows, hints.
    const DIM: Color = Color::Rgb(140, 148, 160);
    /// Deeper dim for secondary hints / empty-state sub-lines.
    const DIMMER: Color = Color::Rgb(90, 98, 112);
    /// Thin rule lines.
    const RULE: Color = Color::Rgb(60, 66, 78);
    /// Focus color: keybind chars, selection bar, active cursor.
    const ACCENT: Color = Color::Rgb(125, 211, 252);
    /// Success flash.
    const OK: Color = Color::Rgb(134, 239, 172);
    /// Error flash.
    const WARN: Color = Color::Rgb(248, 113, 113);
}

fn right_width(line: &Line<'_>) -> u16 {
    line.spans
        .iter()
        .map(|s| s.content.chars().count() as u16)
        .sum::<u16>()
        + 1
}

/// Shorten a long path to fit: keep the leading ~ and the last two
/// segments, collapse the middle with …/
fn compact_path(p: &str, max: usize) -> String {
    if p.chars().count() <= max {
        return p.to_string();
    }
    let tail_chars = max.saturating_sub(2);
    let tail: String = p
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("…{tail}")
}

/// Glyph per tool (actor id), for the list rendering. Uses emoji for
/// the common harnesses + a falling-back text label otherwise. Kept
/// narrow (terminal width of 1-2 cells) so alignment stays clean.
fn tool_glyph(actor_id: &str) -> &'static str {
    let lower = actor_id.to_ascii_lowercase();
    if lower.contains("claude") {
        "💠"
    } else if lower.contains("cursor") {
        "📘"
    } else if lower.contains("codex") {
        "◆"
    } else if lower.contains("openclaw") || lower.contains("claw") {
        "🦞"
    } else if lower.contains("hermes") {
        "🪽"
    } else if lower.contains("cli") {
        "🧠"
    } else if lower.contains("mcp") {
        "🔌"
    } else {
        "·"
    }
}

/// Day bucket for an event's `time_recorded`. Used to group the list
/// into Today / Yesterday / This week / older sections.
fn day_bucket(t: DateTime<Utc>) -> &'static str {
    let now = Utc::now();
    let today = now.date_naive();
    let then = t.date_naive();
    let days = (today - then).num_days();
    if days <= 0 {
        "Today"
    } else if days == 1 {
        "Yesterday"
    } else if days < 7 {
        "This week"
    } else if days < 30 {
        "This month"
    } else {
        "Older"
    }
}

fn type_glyph(t: &brain_types::EventType) -> &'static str {
    use brain_types::EventType::*;
    match t {
        Observe => "●",
        Claim => "◆",
        Lesson => "✦",
        Pref => "◎",
        SkillEdit => "◈",
        Verify => "✓",
        Archive => "▢",
        Redact => "⊘",
        Import => "↓",
        Audit => "·",
    }
}

fn type_color(t: &brain_types::EventType) -> Color {
    use brain_types::EventType::*;
    match t {
        Observe => Color::Rgb(125, 211, 252),   // cyan
        Claim => Color::Rgb(196, 181, 253),     // lavender
        Lesson => Color::Rgb(252, 211, 77),     // amber
        Pref => Color::Rgb(134, 239, 172),      // green
        SkillEdit => Color::Rgb(253, 186, 116), // orange
        Verify => Color::Rgb(134, 239, 172),    // green
        Archive => Color::Rgb(140, 148, 160),   // dim
        Redact => Color::Rgb(248, 113, 113),    // red
        Import => Color::Rgb(196, 181, 253),    // lavender
        Audit => Color::Rgb(140, 148, 160),     // dim
    }
}

fn centered_rect(area: Rect, width_pct: u16, height: u16) -> Rect {
    let w = area.width.saturating_mul(width_pct) / 100;
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

fn short_when(t: DateTime<Utc>) -> String {
    let now = Utc::now();
    let d = now.signed_duration_since(t);
    if d.num_seconds() < 60 {
        "just now".into()
    } else if d.num_minutes() < 60 {
        format!("{}m ago", d.num_minutes())
    } else if d.num_hours() < 24 {
        format!("{}h ago", d.num_hours())
    } else if d.num_days() < 7 {
        format!("{}d ago", d.num_days())
    } else {
        t.format("%b %-d").to_string()
    }
}

fn payload_body(p: &EventPayload) -> String {
    match p {
        EventPayload::Observe(o) => {
            let mut s = o.summary.clone();
            if let Some(c) = &o.content {
                s.push_str("\n\n");
                s.push_str(c);
            }
            if let Some(src) = &o.source {
                s.push_str(&format!("\n\nsource: {src}"));
            }
            s
        }
        EventPayload::Claim(c) => format!(
            "{}/{}\n\n{}",
            c.schema_type,
            c.key,
            serde_json::to_string_pretty(&c.content).unwrap_or_default()
        ),
        EventPayload::Lesson(l) => format!("{} — {}", l.category, l.text),
        EventPayload::Pref(p) => format!(
            "{}/{} = {}",
            p.category,
            p.key,
            serde_json::to_string_pretty(&p.value).unwrap_or_default()
        ),
        EventPayload::SkillEdit(s) => format!(
            "{} {}{}",
            s.skill_name,
            s.file,
            s.reason
                .as_ref()
                .map(|r| format!("\n\n{r}"))
                .unwrap_or_default()
        ),
        EventPayload::Verify(v) => format!("verify {}\n\n{}", v.target_entity, v.attestation),
        EventPayload::Archive(a) => format!(
            "archive {}{}",
            a.target_entity,
            a.reason
                .as_ref()
                .map(|r| format!("\n\n{r}"))
                .unwrap_or_default()
        ),
        EventPayload::Redact(r) => format!("redact {}\n\nreason: {}", r.target_entity, r.reason),
        EventPayload::Import(i) => format!("import {} items\n\n{}", i.item_count, i.manifest_hash),
        EventPayload::Audit(a) => format!("audit {:?}\n\n{}", a.kind, a.detail),
    }
}

#[cfg(test)]
mod tests {
    //! Headless App tests. These exercise state transitions without
    //! touching crossterm / the real terminal, so they run in CI and
    //! from `cargo test` without a tty.
    use super::*;
    use tempfile::TempDir;

    fn app_with_events(n: usize) -> (TempDir, App) {
        let td = TempDir::new().unwrap();
        let brain = LocalBrain::init(td.path().join("b")).unwrap();
        for i in 0..n {
            brain.observe_summary(format!("note {i}")).unwrap();
        }
        let app = App::new(brain);
        (td, app)
    }

    #[test]
    fn new_loads_existing_notes() {
        let (_td, app) = app_with_events(3);
        assert_eq!(app.events.len(), 3);
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn j_and_k_navigate_wrapping_around() {
        let (_td, mut app) = app_with_events(3);
        app.handle_key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.list_state.selected(), Some(1));
        app.handle_key(KeyCode::Char('j'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.list_state.selected(), Some(0), "wrap past end");
        app.handle_key(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(app.list_state.selected(), Some(2), "wrap past start");
    }

    #[test]
    fn g_and_shift_g_jump_to_ends() {
        let (_td, mut app) = app_with_events(5);
        app.handle_key(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(app.list_state.selected(), Some(4));
        app.handle_key(KeyCode::Char('g'), KeyModifiers::NONE);
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn slash_enters_search_mode_and_esc_exits() {
        let (_td, mut app) = app_with_events(2);
        app.handle_key(KeyCode::Char('/'), KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Search);
        app.handle_key(KeyCode::Char('f'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Char('o'), KeyModifiers::NONE);
        assert_eq!(app.search_input, "fo");
        app.handle_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn t_cycles_tool_filter_none_tool_none() {
        // Build a brain with two saved notes (both have actor.id
        // = whoami_or_default(), so the tool_cycle has exactly one
        // entry). Press `t` → filter active. Press `t` again → back
        // to None. events count tracks correctly.
        let (_td, mut app) = app_with_events(2);
        let total = app.all_events.len();
        assert!(total > 0);
        assert!(app.tool_filter.is_none());

        // First `t` → set to the single known tool.
        app.handle_key(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(app.events.len(), total, "single tool → filter matches all");
        assert!(app.tool_filter.is_some());

        // Second `t` → back to None.
        app.handle_key(KeyCode::Char('t'), KeyModifiers::NONE);
        assert!(app.tool_filter.is_none());
        assert_eq!(app.events.len(), total);
    }

    #[test]
    fn day_bucket_labels_today_vs_older() {
        let now = Utc::now();
        assert_eq!(day_bucket(now), "Today");
        assert_eq!(day_bucket(now - chrono::Duration::days(1)), "Yesterday");
        assert_eq!(day_bucket(now - chrono::Duration::days(3)), "This week");
        assert_eq!(day_bucket(now - chrono::Duration::days(15)), "This month");
        assert_eq!(day_bucket(now - chrono::Duration::days(60)), "Older");
    }

    #[test]
    fn tool_glyph_maps_common_harnesses() {
        assert_eq!(tool_glyph("claude-code"), "💠");
        assert_eq!(tool_glyph("cursor-app"), "📘");
        assert_eq!(tool_glyph("codex-cli"), "◆");
        assert_eq!(tool_glyph("openclaw"), "🦞");
        assert_eq!(tool_glyph("hermes-agent"), "🪽");
        assert_eq!(tool_glyph("brain-cli"), "🧠");
        assert_eq!(tool_glyph("something-random"), "·");
    }

    #[test]
    fn n_opens_compose_and_enter_saves_a_note() {
        let (_td, mut app) = app_with_events(1);
        app.handle_key(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Compose);
        for c in "hello tui".chars() {
            app.handle_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Normal, "saving returns to normal");
        assert_eq!(app.events.len(), 2, "list reloaded after save");
        let latest = app
            .events
            .iter()
            .find(|e| matches!(&e.payload, EventPayload::Observe(o) if o.summary == "hello tui"));
        assert!(latest.is_some(), "newly saved note appears in log");
    }

    #[test]
    fn compose_esc_discards_without_saving() {
        let (_td, mut app) = app_with_events(1);
        app.handle_key(KeyCode::Char('n'), KeyModifiers::NONE);
        for c in "discarded".chars() {
            app.handle_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        app.handle_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.events.len(), 1, "no new notes");
        assert!(app.compose_input.is_empty(), "buffer cleared");
    }

    #[test]
    fn compose_empty_enter_flashes_error_and_stays_open() {
        let (_td, mut app) = app_with_events(0);
        app.handle_key(KeyCode::Char('n'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Compose, "empty save keeps modal open");
        assert!(app.flash.as_ref().unwrap().0.starts_with("error:"));
    }

    #[test]
    fn search_narrows_live_without_pressing_enter() {
        // Redesigned behavior: typing narrows the list on every
        // keystroke. Enter just exits Search mode keeping the view.
        let (_td, mut app) = app_with_events(0);
        app.brain.observe_summary("apple pie".into()).unwrap();
        app.brain.observe_summary("pear tart".into()).unwrap();
        app.brain.observe_summary("apple cobbler".into()).unwrap();
        app.reload_log().unwrap();
        assert_eq!(app.events.len(), 3);

        app.handle_key(KeyCode::Char('/'), KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Search);
        for c in "apple".chars() {
            app.handle_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        // No Enter pressed — list should already be narrowed.
        assert_eq!(app.events.len(), 2, "live narrow before Enter");
        assert_eq!(app.mode, Mode::Search, "still in search mode");

        // Enter exits search, keeps narrowed view.
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.events.len(), 2, "narrow persists after Enter");
    }

    #[test]
    fn search_backspace_re_widens_live() {
        let (_td, mut app) = app_with_events(0);
        app.brain.observe_summary("apple".into()).unwrap();
        app.brain.observe_summary("banana".into()).unwrap();
        app.reload_log().unwrap();

        app.handle_key(KeyCode::Char('/'), KeyModifiers::NONE);
        // FTS5 needs exact tokens or `*` suffix for prefix match.
        for c in "apple".chars() {
            app.handle_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!(app.events.len(), 1);

        // Clear the query letter by letter; when empty, full log returns.
        for _ in 0.."apple".len() {
            app.handle_key(KeyCode::Backspace, KeyModifiers::NONE);
        }
        assert_eq!(app.events.len(), 2, "backspace to empty restores full log");
    }

    #[test]
    fn compose_save_failure_preserves_buffer() {
        // Codex R14 P2 regression guard: on save error (simulated by
        // trying to write a secret-bearing note), the compose buffer
        // must not be cleared, so the user can fix and retry.
        let (_td, mut app) = app_with_events(0);
        app.handle_key(KeyCode::Char('n'), KeyModifiers::NONE);
        // Anthropic key pattern — this will trip the secret prefilter
        // and cause observe_summary to return an error.
        for c in "my key is sk-ant-abcdefghijklmnopqrstuvwxyz0123456789abcdef".chars() {
            app.handle_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        let before = app.compose_input.clone();
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Compose, "stay in compose on save error");
        assert_eq!(app.compose_input, before, "buffer preserved for retry");
        assert!(app.flash.as_ref().unwrap().0.starts_with("error:"));
    }

    #[test]
    fn search_enter_narrows_list_to_hits() {
        let (_td, mut app) = app_with_events(0);
        app.brain.observe_summary("apple pie".into()).unwrap();
        app.brain.observe_summary("pear tart".into()).unwrap();
        app.brain.observe_summary("apple cobbler".into()).unwrap();
        app.reload_log().unwrap();
        assert_eq!(app.events.len(), 3);

        app.handle_key(KeyCode::Char('/'), KeyModifiers::NONE);
        for c in "apple".chars() {
            app.handle_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.events.len(), 2, "only apple notes remain");
    }

    #[test]
    fn search_respects_active_tool_filter() {
        let (_td, mut app) = app_with_events(0);
        app.brain.observe_summary("apple pie".into()).unwrap();
        app.reload_log().unwrap();
        assert_eq!(app.events.len(), 1);

        app.tool_filter = Some("nonexistent-tool".into());
        app.search_input = "apple".into();
        app.apply_search();

        assert_eq!(
            app.events.len(),
            0,
            "FTS hits outside the active tool filter should stay hidden"
        );
    }

    #[test]
    fn d_opens_doctor_and_any_key_closes() {
        let (_td, mut app) = app_with_events(2);
        app.handle_key(KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Doctor);
        assert!(app.doctor_report.is_some());
        app.handle_key(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.doctor_report.is_none());
    }

    #[test]
    fn q_quits() {
        let (_td, mut app) = app_with_events(0);
        assert!(!app.quit);
        app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.quit);
    }

    #[test]
    fn ctrl_c_quits_from_any_mode() {
        let (_td, mut app) = app_with_events(0);
        app.handle_key(KeyCode::Char('/'), KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Search);
        app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.quit);
    }

    #[test]
    fn short_when_buckets() {
        let now = Utc::now();
        assert_eq!(short_when(now), "just now");
        assert_eq!(short_when(now - chrono::Duration::minutes(5)), "5m ago");
        assert_eq!(short_when(now - chrono::Duration::hours(3)), "3h ago");
        assert_eq!(short_when(now - chrono::Duration::days(2)), "2d ago");
    }

    #[test]
    fn payload_summary_line_truncates_to_max() {
        // `summary_line` lives in brain-types now, but we keep this
        // test here to guard the shape TUI depends on.
        let p = EventPayload::Observe(brain_types::ObservePayload {
            summary: "a".repeat(200),
            content: None,
            content_ref: None,
            source: None,
        });
        let s = p.summary_line(20);
        assert!(s.chars().count() <= 20);
        assert!(s.ends_with('…'));
    }
}
