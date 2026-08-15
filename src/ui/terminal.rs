use std::{io, panic, sync::Once};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::error::{Error, Result};

static PANIC_HOOK: Once = Once::new();

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

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
        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
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

fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            previous(info);
        }));
    });
}
