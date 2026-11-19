//! What an app's pane will be (v0.16 M3), for the tests that come before
//! it: an app session on the records service, woken, polled and drawn into
//! a buffer the size of a docked app, and read as text.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use bzz::apps::runtime::draw::text_of;
use bzz::apps::runtime::{AppTheme, Key, Signal};
use bzz::apps::session::{AppSession, SessionOptions, SessionStatus};
use bzz::service::records::RecordsService;
use ratatui::{buffer::Buffer, layout::Rect};
use uuid::Uuid;

pub struct Pane {
    pub session: AppSession,
    signal: Signal,
    pub size: (u16, u16),
}

impl Pane {
    /// Opens `app` in `channel`, at the size an app has when it is docked
    /// beside the conversation.
    pub fn open(service: &RecordsService, channel: Uuid, app: &str, source: &str) -> Self {
        Self::open_with(service, channel, source, SessionOptions::new(app))
    }

    pub fn open_with(
        service: &RecordsService,
        channel: Uuid,
        source: &str,
        options: SessionOptions,
    ) -> Self {
        let signal = Signal::new();
        let session =
            AppSession::on_relay(source, options.waker(signal.waker()), service, channel).unwrap();
        Self {
            session,
            signal,
            size: (40, 16),
        }
    }

    pub fn screen(&mut self) -> String {
        self.session.poll();
        let area = Rect::new(0, 0, self.size.0, self.size.1);
        let mut buffer = Buffer::empty(area);
        self.session.draw(area, &mut buffer, &AppTheme::default());
        text_of(&buffer)
    }

    pub fn status(&mut self) -> SessionStatus {
        self.session.poll();
        self.session.status()
    }

    pub fn key(&mut self, written: &str) {
        assert!(self.session.key(Key::parse(written).expect(written)));
    }

    pub fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            let key = if character == ' ' {
                Key::named("space")
            } else {
                Key::named(&character.to_string())
            };
            assert!(self.session.key(key));
        }
    }

    /// Polls and draws until the screen and the status are as wanted. The
    /// screen is returned. Never blocks the test's runtime: the session's
    /// own tasks run while this waits.
    pub async fn until(
        &mut self,
        what: &str,
        mut ready: impl FnMut(&str, &SessionStatus) -> bool,
    ) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let screen = self.screen();
            let status = self.session.status();
            if ready(&screen, &status) {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}:\n{screen}\n{status:?}"
            );
            tokio::time::sleep(Duration::from_millis(3)).await;
            // The owner was woken, or its own tick has come.
            self.signal.wait(Duration::ZERO);
            self.session.tick();
        }
    }

    /// The screen once the app's own status line, its last row, says
    /// `status`.
    pub async fn settled(&mut self, status: &str) -> String {
        self.until(status, |screen, _| last_row(screen) == status)
            .await
    }
}

pub fn last_row(screen: &str) -> &str {
    screen.lines().last().unwrap_or("")
}
