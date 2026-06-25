use std::time::Duration;

use arboard::Clipboard;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use emojis::Group;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Paragraph};
use ratatui::DefaultTerminal;
use unicode_width::UnicodeWidthStr;

/// One cell-width of the emoji grid, in terminal columns.
/// A leading space, the (assumed 2-wide) emoji, then padding.
const CELL_W: u16 = 4;

/// Emoji groups in their canonical display order, with friendly labels.
const GROUPS: [(Group, &str); 9] = [
    (Group::SmileysAndEmotion, "Smileys & Emotion"),
    (Group::PeopleAndBody, "People & Body"),
    (Group::AnimalsAndNature, "Animals & Nature"),
    (Group::FoodAndDrink, "Food & Drink"),
    (Group::TravelAndPlaces, "Travel & Places"),
    (Group::Activities, "Activities"),
    (Group::Objects, "Objects"),
    (Group::Symbols, "Symbols"),
    (Group::Flags, "Flags"),
];

struct EmojiItem {
    ch: &'static str,
    name: String,
    group: Group,
    /// Lowercased name + shortcodes, used as the fuzzy-search haystack.
    haystack: String,
}

/// A category section: a header label and the emoji that belong to it.
struct GroupBlock {
    name: &'static str,
    /// Indices into `App::items`, in display order.
    items: Vec<usize>,
}

type ScoredBuckets<'a> = Vec<(u32, &'a str, Vec<(u32, usize)>)>;

struct App {
    items: Vec<EmojiItem>,
    query: String,
    /// Visible category sections, in display order.
    groups: Vec<GroupBlock>,
    /// Flattened selectable emoji (concatenation of every block's items).
    selectable: Vec<usize>,
    /// Geometry of emoji grid rows as (first selectable index, length).
    nav_rows: Vec<(usize, usize)>,
    selected: usize,
    /// Scroll offset, in visual rows (headers included).
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
                    group: e.group(),
                    haystack,
                }
            })
            .collect();

        let mut app = App {
            items,
            query: String::new(),
            groups: Vec::new(),
            selectable: Vec::new(),
            nav_rows: Vec::new(),
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
        let mut blocks: Vec<GroupBlock> = Vec::new();

        if query.is_empty() {
            // Browse mode: every group in canonical order.
            for (group, name) in GROUPS {
                let items: Vec<usize> = self
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(_, it)| it.group == group)
                    .map(|(i, _)| i)
                    .collect();
                if !items.is_empty() {
                    blocks.push(GroupBlock { name, items });
                }
            }
        } else {
            // Search mode: fuzzy-match across everything, then bucket the
            // matches by group. Groups are ordered by their best match so the
            // single most relevant emoji still sits at the very top.
            let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
            let mut buf = Vec::new();
            let mut buckets: ScoredBuckets = GROUPS
                .iter()
                .map(|(_, name)| (0u32, *name, Vec::new()))
                .collect();

            for (i, item) in self.items.iter().enumerate() {
                let hs = Utf32Str::new(&item.haystack, &mut buf);
                if let Some(score) = pattern.score(hs, &mut self.matcher) {
                    let gi = GROUPS.iter().position(|(g, _)| *g == item.group).unwrap();
                    buckets[gi].2.push((score, i));
                    buckets[gi].0 = buckets[gi].0.max(score);
                }
            }

            buckets.retain(|(_, _, v)| !v.is_empty());
            buckets.sort_by(|a, b| b.0.cmp(&a.0));
            for (_, name, mut v) in buckets {
                v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                blocks.push(GroupBlock {
                    name,
                    items: v.into_iter().map(|(_, i)| i).collect(),
                });
            }
        }

        self.groups = blocks;
        self.selectable = self
            .groups
            .iter()
            .flat_map(|b| b.items.iter().copied())
            .collect();
        self.selected = 0;
        self.offset = 0;
    }

    /// Recompute grid-row geometry for the current column count.
    fn rebuild_nav(&mut self, cols: usize) {
        let cols = cols.max(1);
        self.columns = cols;
        let mut rows = Vec::new();
        let mut flat = 0;
        for block in &self.groups {
            let n = block.items.len();
            let mut i = 0;
            while i < n {
                let len = (n - i).min(cols);
                rows.push((flat + i, len));
                i += len;
            }
            flat += n;
        }
        self.nav_rows = rows;
    }

    fn current_nav_row(&self) -> Option<usize> {
        self.nav_rows
            .iter()
            .position(|&(start, len)| self.selected >= start && self.selected < start + len)
    }

    fn move_selection(&mut self, dx: isize, dy: isize) {
        if self.selectable.is_empty() {
            return;
        }
        if dx != 0 {
            let len = self.selectable.len() as isize;
            self.selected = (self.selected as isize + dx).clamp(0, len - 1) as usize;
            return;
        }
        if dy != 0 {
            if let Some(ri) = self.current_nav_row() {
                let (start, _) = self.nav_rows[ri];
                let col = self.selected - start;
                let target = ri as isize + dy;
                if target >= 0 && (target as usize) < self.nav_rows.len() {
                    let (tstart, tlen) = self.nav_rows[target as usize];
                    self.selected = tstart + col.min(tlen - 1);
                }
            }
        }
    }

    fn copy_selected(&mut self) {
        if self.selectable.is_empty() {
            return;
        }
        let item = &self.items[self.selectable[self.selected]];
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
    f.set_cursor_position((chunks[0].x + 2 + app.query.width() as u16, chunks[0].y + 1));

    // --- Emoji grid (grouped) ---
    let grid = chunks[1];
    let cols = (grid.width / CELL_W).max(1) as usize;
    app.rebuild_nav(cols);

    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let pad_to = CELL_W as usize;

    let rule_width = grid.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut sel_vis_row = 0usize;
    let mut flat = 0usize;
    for block in &app.groups {
        let label = format!("── {} ", block.name);
        let trailing = rule_width.saturating_sub(label.width());
        lines.push(Line::from(Span::styled(
            format!("{}{}", label, "─".repeat(trailing)),
            header_style,
        )));
        let n = block.items.len();
        let mut i = 0;
        while i < n {
            let len = (n - i).min(cols);
            let mut spans: Vec<Span> = Vec::with_capacity(len);
            for j in 0..len {
                let fi = flat + i + j;
                let item = &app.items[block.items[i + j]];
                let w = item.ch.width().min(pad_to - 1);
                let cell = format!(" {}{}", item.ch, " ".repeat(pad_to - 1 - w));
                let style = if fi == app.selected {
                    sel_vis_row = lines.len();
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(cell, style));
            }
            lines.push(Line::from(spans));
            i += len;
        }
        flat += n;
    }

    // Keep the selected row (and the header above it) inside the viewport.
    let rows_visible = grid.height as usize;
    if sel_vis_row <= app.offset {
        app.offset = sel_vis_row.saturating_sub(1);
    } else if rows_visible > 0 && sel_vis_row >= app.offset + rows_visible {
        app.offset = sel_vis_row + 1 - rows_visible;
    }
    let start = app.offset.min(lines.len());
    let end = (start + rows_visible).min(lines.len());
    f.render_widget(Paragraph::new(lines[start..end].to_vec()), grid);

    // --- Status bar ---
    let status_chunks =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(34)]).split(chunks[2]);

    let left = if let Some(s) = &app.status {
        s.clone()
    } else if app.selectable.is_empty() {
        " no matches".to_string()
    } else {
        let item = &app.items[app.selectable[app.selected]];
        format!(
            " {}  {}   ·   {}/{}",
            item.ch,
            item.name,
            app.selected + 1,
            app.selectable.len()
        )
    };
    f.render_widget(Paragraph::new(left), status_chunks[0]);
    f.render_widget(
        Paragraph::new("⏎ copy  ↑↓←→ move  esc clear/quit")
            .style(Style::default().add_modifier(Modifier::DIM))
            .right_aligned(),
        status_chunks[1],
    );
}
