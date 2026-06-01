# emoj-tui

A tiny, fast terminal emoji picker. Type to filter, arrow keys to move, **Enter to copy** to the clipboard. No ads, no network, no telemetry — inspired by [letsemoji.com](https://letsemoji.com).

```
┌ 🔍  search emoji ──────────────────────────┐
│ heart                                      │
└────────────────────────────────────────────┘
 ❤  🧡  💛  💚  💙  💜  🤎  🖤  🤍  💔  ❣  💕
 💞  💓  💗  💖  💘  💝  💟  ♥  …

 ❤  red heart   ·   1/42        ⏎ copy  ↑↓←→ move  esc clear/quit
```

## Build & run

Requires the Rust toolchain ([rustup](https://rustup.rs)).

```sh
cargo run --release          # run in place
cargo install --path .       # install `emoji` onto your PATH
```

Then just run:

```sh
emoji
```

## Keys

| Key            | Action                          |
| -------------- | ------------------------------- |
| any character  | filter (fuzzy match)            |
| `↑ ↓ ← →`      | move selection                  |
| `Enter`        | copy selected emoji to clipboard|
| `Esc`          | clear search, or quit if empty  |
| `Ctrl-C` / `Ctrl-Q` | quit                       |

## How it works

All ~3,700 emoji ship compiled into the binary via the [`emojis`](https://crates.io/crates/emojis) crate (names + GitHub shortcodes). Emoji are organized under category headers (Smileys & Emotion, People & Body, Animals & Nature, …). Search is fuzzy-matched and ranked in-memory with [`nucleo-matcher`](https://crates.io/crates/nucleo-matcher), so every keystroke re-filters instantly; matches stay grouped by category, with the category holding the best match shown first. The UI is [`ratatui`](https://ratatui.rs); clipboard is [`arboard`](https://crates.io/crates/arboard).
