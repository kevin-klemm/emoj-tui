use std::time::Duration;

use arboard::Clipboard;
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Paragraph};
use ratatui::DefaultTerminal;
use unicode_width::UnicodeWidthStr;

/// One cell-width of the emoji grid, in terminal columns.
/// A leading space, the (assumed 2-wide) emoji, then padding.
const CELL_W: u16 = 4;

struct EmojiItem {
    ch: &'static str,
    name: String,
    /// Lowercased name + shortcodes, used as the fuzzy-search haystack.
    haystack: String,
}

struct App {
    items: Vec<EmojiItem>,
    query: String,
    /// Indices into `items`, in display order (best match first).
    filtered: Vec<usize>,
    selected: usize,
    offset: usize,
    columns: usize,
    matcher: Matcher,
    status: Option<String>,
    clipboard: Option<Clipboard>,
}

impl App {
    fn new() -> Self {
        let items = emojis::iter()
            .map(|e| {
                let mut haystack = e.name().to_lowercase();
                for sc in e.shortcodes() {
                    haystack.push(' ');
                    haystack.push_str(sc);
                }
                EmojiItem {
                    ch: e.as_str(),
                    name: e.name().to_string(),
                    haystack,
                }
            })
            .collect();

        let mut app = App {
            items,
            query: String::new(),
            filtered: Vec::new(),
            selected: 0,
            offset: 0,
            columns: 1,
            matcher: Matcher::new(Config::DEFAULT),
            status: None,
            clipboard: Clipboard::new().ok(),
        };
        app.refilter();
        app
    }

    fn refilter(&mut self) {
        let query = self.query.trim();
        if query.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
            let mut buf = Vec::new();
            let mut scored: Vec<(u32, usize)> = Vec::new();
            for (i, item) in self.items.iter().enumerate() {
                let hs = Utf32Str::new(&item.haystack, &mut buf);
                if let Some(score) = pattern.score(hs, &mut self.matcher) {
                    scored.push((score, i));
                }
            }
            // Higher score first; ties keep the original (Unicode) order.
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        }
        self.selected = 0;
        self.offset = 0;
    }

    fn move_selection(&mut self, dx: isize, dy: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let cols = self.columns.max(1) as isize;
        let len = self.filtered.len() as isize;
        let mut idx = self.selected as isize;
        if dx != 0 {
            idx = (idx + dx).clamp(0, len - 1);
        }
        if dy != 0 {
            let next = idx + dy * cols;
            if next >= 0 && next < len {
                idx = next;
            }
        }
        self.selected = idx as usize;
    }

    fn copy_selected(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let item = &self.items[self.filtered[self.selected]];
        let ch = item.ch.to_string();
        let name = item.name.clone();
        if self.clipboard.is_none() {
            self.clipboard = Clipboard::new().ok();
        }
        let ok = self
            .clipboard
            .as_mut()
            .map(|cb| cb.set_text(ch.clone()).is_ok())
            .unwrap_or(false);
        self.status = Some(if ok {
            format!("✓ copied {}  {}", ch, name)
        } else {
            "✗ clipboard unavailable".into()
        });
    }
}

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('q') if ctrl => return Ok(()),
                KeyCode::Esc => {
                    if app.query.is_empty() {
                        return Ok(());
                    }
                    app.query.clear();
                    app.refilter();
                    app.status = None;
                }
                KeyCode::Enter => app.copy_selected(),
                KeyCode::Backspace => {
                    app.query.pop();
                    app.refilter();
                    app.status = None;
                }
                KeyCode::Left => {
                    app.move_selection(-1, 0);
                    app.status = None;
                }
                KeyCode::Right => {
                    app.move_selection(1, 0);
                    app.status = None;
                }
                KeyCode::Up => {
                    app.move_selection(0, -1);
                    app.status = None;
                }
                KeyCode::Down => {
                    app.move_selection(0, 1);
                    app.status = None;
                }
                KeyCode::Char(c) if !ctrl => {
                    app.query.push(c);
                    app.refilter();
                    app.status = None;
                }
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(f.area());

    // --- Search box ---
    let search = Paragraph::new(format!(" {}", app.query))
        .block(Block::bordered().title(" 🔍  search emoji "));
    f.render_widget(search, chunks[0]);
    f.set_cursor_position((
        chunks[0].x + 2 + app.query.width() as u16,
        chunks[0].y + 1,
    ));

    // --- Emoji grid ---
    let grid = chunks[1];
    let cols = (grid.width / CELL_W).max(1) as usize;
    app.columns = cols;
    let rows_visible = grid.height as usize;

    // Keep the selection inside the viewport.
    let sel_row = app.selected / cols;
    if sel_row < app.offset {
        app.offset = sel_row;
    } else if rows_visible > 0 && sel_row >= app.offset + rows_visible {
        app.offset = sel_row + 1 - rows_visible;
    }

    let pad_to = CELL_W as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(rows_visible);
    for r in 0..rows_visible {
        let row = app.offset + r;
        let mut spans: Vec<Span> = Vec::with_capacity(cols);
        for c in 0..cols {
            let fi = row * cols + c;
            if fi >= app.filtered.len() {
                break;
            }
            let item = &app.items[app.filtered[fi]];
            let w = item.ch.width().min(pad_to - 1);
            let trailing = pad_to - 1 - w;
            let cell = format!(" {}{}", item.ch, " ".repeat(trailing));
            let style = if fi == app.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            spans.push(Span::styled(cell, style));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), grid);

    // --- Status bar ---
    let status_chunks =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(40)]).split(chunks[2]);

    let left = if let Some(s) = &app.status {
        s.clone()
    } else if app.filtered.is_empty() {
        "no matches".to_string()
    } else {
        let item = &app.items[app.filtered[app.selected]];
        format!(
            " {}  {}   ·   {}/{}",
            item.ch,
            item.name,
            app.selected + 1,
            app.filtered.len()
        )
    };
    let dim = Style::default().add_modifier(Modifier::DIM);
    f.render_widget(Paragraph::new(left), status_chunks[0]);
    f.render_widget(
        Paragraph::new("⏎ copy  ↑↓←→ move  esc clear/quit")
            .style(dim)
            .right_aligned(),
        status_chunks[1],
    );
}
