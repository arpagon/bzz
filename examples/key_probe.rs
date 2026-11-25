//! Prints the terminal events crossterm delivers, one per line, so that the
//! keys of one platform can be compared with another's (docs/e2e-windows.md).
//!
//! `cargo run --example key_probe -- [seconds]` enters raw mode with bracketed
//! paste and mouse capture, prints each event as crossterm reports it and
//! next to it what bzz makes of it, and leaves after the given seconds
//! (default 120) or on F12. It reads nothing but the terminal and writes
//! nothing but the terminal.

use std::io::Write;
use std::time::{Duration, Instant};

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    KeyCode,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

fn main() -> std::io::Result<()> {
    let seconds = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(120);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut out = std::io::stdout();
    enable_raw_mode()?;
    crossterm::execute!(out, EnableBracketedPaste, EnableMouseCapture)?;
    write!(
        out,
        "key_probe {} for {seconds}s; F12 leaves\r\n",
        std::env::consts::OS
    )?;
    out.flush()?;
    let mut number = 0_u32;
    while Instant::now() < deadline {
        if !crossterm::event::poll(Duration::from_millis(100))? {
            continue;
        }
        let event = crossterm::event::read()?;
        number += 1;
        let line = match &event {
            Event::Key(key) => {
                let normal = bzz::ui::keymap::normalize_key_event(*key);
                format!(
                    "{number} key code={:?} mods={:?} kind={:?} state={:?} -> bzz code={:?} mods={:?}",
                    key.code, key.modifiers, key.kind, key.state, normal.code, normal.modifiers
                )
            }
            Event::Paste(text) => format!(
                "{number} paste chars={} lines={} text={:?}",
                text.chars().count(),
                text.lines().count(),
                text
            ),
            Event::Mouse(mouse) => format!(
                "{number} mouse kind={:?} col={} row={} mods={:?}",
                mouse.kind, mouse.column, mouse.row, mouse.modifiers
            ),
            Event::Resize(columns, rows) => format!("{number} resize {columns}x{rows}"),
            other => format!("{number} other {other:?}"),
        };
        write!(out, "{line}\r\n")?;
        out.flush()?;
        if matches!(event, Event::Key(key) if key.code == KeyCode::F(12)) {
            break;
        }
    }
    crossterm::execute!(out, DisableMouseCapture, DisableBracketedPaste)?;
    disable_raw_mode()?;
    Ok(())
}
