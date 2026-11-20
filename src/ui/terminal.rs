use std::{
    io::{self, Write as _},
    panic,
    sync::Once,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::error::{Error, Result};

static PANIC_HOOK: Once = Once::new();

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// Upper bound for one explicit OSC 52 copy. It limits terminal output as well
/// as accidental clipboard retention of an unexpectedly large message range.
pub const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;

/// Builds a control-safe OSC 52 command. The payload is base64 only; user text
/// is never interpolated into a terminal control sequence.
pub fn osc52_sequence(text: &str) -> Result<Vec<u8>> {
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err(Error::Unsupported(format!(
            "copy exceeds the {} KiB clipboard safety limit",
            MAX_CLIPBOARD_BYTES / 1024
        )));
    }
    let encoded = STANDARD.encode(text.as_bytes());
    Ok(format!("\x1b]52;c;{encoded}\x07").into_bytes())
}

/// Copies text only after an explicit user action. Callers provide already
/// sanitized source text; this method never prints that text in a status line.
pub fn copy_osc52(text: &str) -> Result<usize> {
    let sequence = osc52_sequence(text)?;
    let mut stdout = io::stdout();
    stdout
        .write_all(&sequence)
        .map_err(|error| Error::io("terminal clipboard", error))?;
    stdout
        .flush()
        .map_err(|error| Error::io("terminal clipboard", error))?;
    Ok(text.len())
}

pub struct TerminalGuard {
    active: bool,
    mouse_capture: bool,
}

impl TerminalGuard {
    pub fn enter(mouse_capture: bool) -> Result<(Self, Tui)> {
        install_panic_hook();
        enable_raw_mode().map_err(|error| Error::io("terminal", error))?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(Error::io("terminal", error));
        }
        if mouse_capture && let Err(error) = execute!(stdout, EnableMouseCapture) {
            let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(Error::io("terminal", error));
        }
        // A paste arrives as one piece of text, not as keys (v0.16): a line
        // break in it no longer sends a message, and the rest no longer runs
        // as bzz's own keys. A terminal that does not know the mode ignores
        // it. On Windows crossterm gives no paste event: there a paste still
        // arrives as keys.
        let _ = execute!(stdout, EnableBracketedPaste);
        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = execute!(io::stdout(), DisableBracketedPaste);
                let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                return Err(Error::io("terminal", error));
            }
        };
        Ok((
            Self {
                active: true,
                mouse_capture,
            },
            terminal,
        ))
    }

    pub fn restore(&mut self) {
        if self.active {
            if self.mouse_capture {
                let _ = execute!(io::stdout(), DisableMouseCapture);
            }
            // Apart: on a Windows console without escape sequences crossterm
            // refuses this one, and `execute!` stops at the first refusal.
            let _ = execute!(io::stdout(), DisableBracketedPaste);
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            let _ = disable_raw_mode();
            self.active = false;
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// The name of an app's thread starts so (`apps::runtime`).
const APP_THREAD_PREFIX: &str = "bzz-app ";

/// Whether a panic on a thread of this name is an app's. The runtime
/// catches it there and the app says so in its own pane; bzz goes on
/// drawing, so the terminal must be left as it is, and nothing may be
/// printed over the screen.
pub fn is_app_thread(name: Option<&str>) -> bool {
    name.is_some_and(|name| name.starts_with(APP_THREAD_PREFIX))
}

fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if is_app_thread(std::thread::current().name()) {
                return;
            }
            let _ = execute!(io::stdout(), DisableBracketedPaste);
            let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            previous(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::{MAX_CLIPBOARD_BYTES, is_app_thread, osc52_sequence};

    /// The runtime names an app's thread `bzz-app <script>`; the hook
    /// must know it by that name and no other.
    #[test]
    fn a_panic_on_an_apps_thread_leaves_the_terminal_alone() {
        assert!(is_app_thread(Some("bzz-app board.lua")));
        assert!(!is_app_thread(Some("main")));
        assert!(!is_app_thread(Some("tokio-runtime-worker")));
        assert!(!is_app_thread(None));
    }

    #[test]
    fn osc52_encodes_text_without_interpolating_terminal_controls() {
        let sequence = osc52_sequence("hello\u{1b}]unsafe").unwrap();
        assert!(sequence.starts_with(b"\x1b]52;c;"));
        assert!(sequence.ends_with(b"\x07"));
        assert!(!sequence[7..sequence.len() - 1].contains(&0x1b));
    }

    #[test]
    fn osc52_refuses_oversized_content() {
        assert!(osc52_sequence(&"x".repeat(MAX_CLIPBOARD_BYTES + 1)).is_err());
    }
}
