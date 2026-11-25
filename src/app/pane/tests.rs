//! The app pane inside bzz's own `App`, drawn to Ratatui's test backend:
//! an app on records in memory, keys, a paste, the layout and the mouse,
//! with no terminal and no relay.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    config::Config,
    domain::{Channel, ChannelFacts, ChannelKind, Visibility},
    paths::Paths,
    store::{Store, writer::StoreHandle},
    ui::{
        hit_map::HitTarget,
        layout::AppPlacement,
        state::{ComposerTarget, FocusSurface, Overlay},
    },
};

use super::super::App;

/// An app that writes down every key and paste it is given.
const RECORDER: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init(ctx) return { keys = {}, pastes = {} } end
function app.view(s, ctx)
  local pasted = table.concat(s.pastes, "|"):gsub("\n", "/")
  return ui.text { "keys " .. table.concat(s.keys, " "), "pasted " .. pasted, "size " .. ctx.width .. "x" .. ctx.height }
end
function app.key(s, key, ctx) s.keys[#s.keys + 1] = key.name end
function app.paste(s, text, ctx) s.pastes[#s.pastes + 1] = text end
return app
"#;

/// An app whose every key runs until it is stopped.
const LOOPER: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init(ctx) return { n = 0 } end
function app.view(s, ctx) return ui.text { "loops " .. s.n } end
function app.key(s, key, ctx) s.n = s.n + 1 while true do end end
return app
"#;

async fn fixture(temporary: &TempDir) -> (App, Uuid) {
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let config = Config::default();
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    let handle = StoreHandle::spawn(store).unwrap();
    let mut app = App::new(config, paths, handle).await.unwrap();
    let identity = crate::config::IdentityConfig {
        id: Uuid::new_v4(),
        label: "pane".into(),
        pubkey: "a".repeat(64),
        backend: crate::config::KeyBackend::Keychain,
        key_ref: "identity:pane".into(),
    };
    app.config.identities.push(identity.clone());
    app.config
        .add_community(
            "pane".into(),
            "wss://pane.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    let channel = Uuid::new_v4();
    app.channels = vec![
        Channel {
            id: channel,
            name: "general".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 2,
            last_event_at: None,
            facts: ChannelFacts::default(),
        },
        Channel {
            id: Uuid::new_v4(),
            name: "random".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 2,
            last_event_at: None,
            facts: ChannelFacts::default(),
        },
    ];
    app.selected_channel = 0;
    let synced = app.config.clone();
    app.store
        .call(move |store| store.sync_config(&synced))
        .await
        .unwrap();
    (app, channel)
}

fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Polls and draws until the screen shows `wanted`.
async fn until(app: &mut App, width: u16, height: u16, wanted: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_apps();
        let shown = screen(app, width, height);
        if shown.contains(wanted) {
            return shown;
        }
        assert!(Instant::now() < deadline, "no {wanted:?} on:\n{shown}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl_c() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

#[tokio::test]
async fn keys_and_a_paste_reach_the_focused_app_and_nothing_else() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "recorder", RECORDER);
    app.focus_app();
    let shown = until(&mut app, 120, 30, "keys").await;
    assert!(shown.contains("┌ recorder ─"), "{shown}");
    assert!(shown.contains("Ctrl-c leave ┘"), "{shown}");
    assert!(shown.contains(" APP "), "{shown}");
    assert!(shown.contains("Ctrl-c leaves the app"), "{shown}");
    for code in [
        KeyCode::Char('q'),
        KeyCode::Char(':'),
        KeyCode::Char('1'),
        KeyCode::Char('5'),
        KeyCode::Char('i'),
        KeyCode::Char('?'),
        KeyCode::Esc,
        KeyCode::Tab,
        KeyCode::Enter,
    ] {
        assert!(app.app_key(key(code)), "{code:?} is the app's");
    }
    let shown = until(&mut app, 120, 30, "keys q : 1 5 i ? esc tab enter").await;
    // None of them opened anything of bzz's, or asked to quit.
    assert_eq!(app.presentation.overlay, None);
    assert_eq!(app.presentation.composer_target, None);
    assert!(!app.should_quit);
    assert_eq!(app.presentation.focus, FocusSurface::App);
    assert!(!shown.contains("quit bzz?"));
    // A paste of two lines is one piece of text, with LF.
    app.handle_terminal_paste("line one\rline two".into())
        .await
        .unwrap();
    until(&mut app, 120, 30, "pasted line one/line two").await;
    // Ctrl+C leaves, and is not the app's.
    assert!(app.app_key(ctrl_c()));
    assert_eq!(app.presentation.focus, FocusSurface::Timeline);
    assert!(!app.app_has_focus());
    let shown = screen(&mut app, 120, 30);
    assert!(shown.contains("┌ recorder ─"), "{shown}");
    assert!(shown.contains("5 focus ┘"), "{shown}");
    assert!(!shown.contains("Ctrl-c leaves the app"), "{shown}");
    // v0.17 C10: the hint follows the focus and ends in the palette's key.
    assert!(shown.contains("Ctrl-K actions"), "{shown}");
    // Now keys are bzz's again, and the app is given none.
    assert!(!app.app_key(key(KeyCode::Char('x'))));
    // The writing dock owns the keyboard before the app, even focused.
    app.focus_app();
    app.presentation.composer_target = Some(ComposerTarget {
        community_id: app.active_community_id().unwrap(),
        channel_id: channel,
        thread_root_id: None,
        parent_event_id: None,
    });
    assert!(!app.app_key(key(KeyCode::Char('h'))));
    assert!(!app.app_key(ctrl_c()));
    app.handle_terminal_paste("to the\rcomposer".into())
        .await
        .unwrap();
    assert_eq!(app.composer.body, "to the\ncomposer");
    // An overlay owns it too; a paste there goes nowhere.
    app.presentation.composer_target = None;
    app.presentation.open_overlay(Overlay::Help);
    assert!(!app.app_key(key(KeyCode::Char('j'))));
    app.handle_terminal_paste("dropped".into()).await.unwrap();
    app.presentation.close_overlay();
    let shown = until(&mut app, 120, 30, "pasted line one/line two").await;
    assert!(!shown.contains("dropped"), "{shown}");
    assert!(
        shown.contains("keys q : 1 5 i ? esc tab enter\n")
            || shown.contains("keys q : 1 5 i ? esc tab enter "),
        "{shown}"
    );
    assert!(
        !shown.contains("enter h") && !shown.contains("enter x") && !shown.contains("enter j"),
        "{shown}"
    );
}

#[tokio::test]
async fn the_pane_follows_the_window_and_its_channel() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "recorder", RECORDER);
    app.focus_app();
    // 80 by 24: beside the conversation, 40 by 18 inside.
    let shown = until(&mut app, 80, 24, "size 40x18").await;
    assert_eq!(app.apps.placement, AppPlacement::Docked);
    assert!(!shown.contains("communities"), "{shown}");
    // 60 columns: the app takes the window while it has the focus.
    let shown = until(&mut app, 60, 24, "size 58x21").await;
    assert_eq!(app.apps.placement, AppPlacement::Zoomed);
    assert!(!shown.contains("┌ general"), "{shown}");
    // Without the focus it is hidden, and the status line says so.
    app.leave_app();
    let shown = screen(&mut app, 60, 24);
    assert_eq!(app.apps.placement, AppPlacement::HiddenNarrow);
    assert!(shown.contains("app hidden · 5"), "{shown}");
    // Zoom on request, at any size.
    app.zoom_app();
    until(&mut app, 160, 40, "size 158x37").await;
    assert_eq!(app.apps.placement, AppPlacement::Zoomed);
    app.app_key(ctrl_c());
    let shown = screen(&mut app, 160, 40);
    assert_eq!(app.apps.placement, AppPlacement::Docked);
    assert!(shown.contains("communities"), "{shown}");
    // Under 40 by 16 inside bzz says so, and the script draws nothing.
    app.focus_app();
    let shown = screen(&mut app, 120, 18);
    assert!(shown.contains("This app needs 40×16"), "{shown}");
    assert!(!shown.contains("size 40x"), "{shown}");
    // Another channel selected: its app, or none, and the focus comes back.
    app.selected_channel = 1;
    let shown = screen(&mut app, 120, 30);
    assert_eq!(app.apps.placement, AppPlacement::Closed);
    assert_eq!(app.presentation.focus, FocusSurface::Timeline);
    assert!(!shown.contains("recorder"), "{shown}");
    // And the session kept its state for when the owner comes back.
    app.selected_channel = 0;
    until(&mut app, 120, 30, "5 focus ┘").await;
}

#[tokio::test]
async fn the_thread_and_the_app_share_the_right_side() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "recorder", RECORDER);
    app.thread_root = Some("e".repeat(64));
    app.apps.front = false;
    // Asked for last, the thread has the right side under 144 columns, and
    // a reply goes into it.
    let shown = screen(&mut app, 120, 30);
    assert_eq!(app.apps.placement, AppPlacement::HiddenBehindThread);
    assert!(shown.contains("app behind thread · 5"), "{shown}");
    assert!(app.thread_drawn);
    assert_eq!(app.reply_root(), app.thread_root);
    assert!(shown.contains("reply in #general"), "{shown}");
    // `5` brings the app to the front; the thread waits, and the composer
    // no longer replies into it.
    app.focus_app();
    let shown = screen(&mut app, 120, 30);
    assert_eq!(app.apps.placement, AppPlacement::Docked);
    assert!(!app.thread_drawn);
    assert_eq!(app.reply_root(), None);
    assert!(shown.contains("message #general"), "{shown}");
    // From 144 both show.
    screen(&mut app, 144, 30);
    assert!(app.thread_drawn);
    assert_eq!(app.apps.placement, AppPlacement::Docked);
}

#[tokio::test]
async fn the_mouse_focuses_the_pane_and_the_wheel_keeps_the_apps_focus() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "recorder", RECORDER);
    screen(&mut app, 120, 30);
    let map = app.last_hit_map.clone().unwrap();
    // The pane is 42 columns at the far right, above the dock.
    assert_eq!(map.hit(100, 10), Some(&HitTarget::App));
    assert_eq!(map.hit(78, 10), Some(&HitTarget::App));
    assert_eq!(map.hit(77, 10), Some(&HitTarget::Timeline));
    // A focused app keeps the keyboard when the wheel scrolls another pane.
    app.focus_app();
    app.wheel_over(HitTarget::Timeline, 3);
    assert_eq!(app.presentation.focus, FocusSurface::App);
    // The wheel over the app does nothing, and moves no focus.
    app.leave_app();
    app.wheel_over(HitTarget::App, 3);
    assert_eq!(app.presentation.focus, FocusSurface::Timeline);
    // Without an app focused the wheel moves the focus, as in v0.15.
    app.wheel_over(HitTarget::ChannelPane, 3);
    assert_eq!(app.presentation.focus, FocusSurface::Channels);
}

/// A script stuck in a call costs the conversation nothing: keys are taken
/// and frames drawn at once, and the pane says the app does not respond.
#[tokio::test]
async fn a_looping_script_costs_the_window_nothing() {
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.open_app_in_memory(channel, "looper", LOOPER);
    app.focus_app();
    until(&mut app, 120, 30, "loops 0").await;
    let started = Instant::now();
    for _ in 0..20 {
        app.app_key(key(KeyCode::Char('x')));
        app.poll_apps();
        screen(&mut app, 120, 30);
    }
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "{:?} for twenty keys and frames",
        started.elapsed()
    );
    // The composer types while the script loops.
    app.leave_app();
    app.presentation.composer_target = Some(ComposerTarget {
        community_id: app.active_community_id().unwrap(),
        channel_id: channel,
        thread_root_id: None,
        parent_event_id: None,
    });
    let typing = Instant::now();
    for character in "still typing".chars() {
        app.insert_action(crate::ui::keymap::KeyAction::Character(character))
            .await
            .unwrap();
        screen(&mut app, 120, 30);
    }
    assert_eq!(app.composer.body, "still typing");
    assert!(
        typing.elapsed() < Duration::from_millis(1500),
        "{:?}",
        typing.elapsed()
    );
}

// ------------------------------------------------ approval and management

mod managed {
    use super::*;
    use crate::apps::runtime::{Limits, SystemClock, Who};
    use crate::apps::session::{MemoryStore, SharedStore};
    use crate::apps::store::{
        Apps, Outcome, Proposer, Recorded, Submission, VersionState, examine_with,
    };

    /// A counter whose state is its count; each version says which it is.
    fn counter(version: &str, types: &str, extra: &str) -> String {
        format!(
            r#"
local app = {{ api = 1, types = {{ {types} }} }}
function app.init(ctx) return {{ n = 0 }} end
function app.view(s, ctx) return ui.text {{ "{version} count " .. s.n }} end
function app.key(s, key, ctx) if key.name == "j" then s.n = s.n + 1 end end
{extra}
return app
"#
        )
    }

    const BRIEF: &str = "# Counter\n\nCounts. Records: `note`, `tag`, none written.\n";

    /// bzz's App with a store of apps on disk and records in memory.
    async fn managed(temporary: &TempDir) -> (App, Uuid) {
        let (mut app, channel) = fixture(temporary).await;
        app.apps.memory = Some(SharedStore::new(MemoryStore::new(
            Who::new("a".repeat(64), "you"),
            SystemClock::shared(),
        )));
        app.tick_apps().await.unwrap();
        (app, channel)
    }

    /// Proposes a version, as `bzz app add` or an agent's tool does.
    async fn propose(app: &App, script: String, from: Proposer) -> Recorded {
        let community = app.active_community_id().unwrap();
        let root = app.paths.apps_dir();
        let submission = Submission::new("counter", script, BRIEF, from);
        let examined =
            tokio::task::spawn_blocking(move || examine_with(submission, &Limits::patient()))
                .await
                .unwrap()
                .unwrap();
        app.store
            .call(move |store| Ok(Apps::at(store, root).record(community, examined).unwrap()))
            .await
            .unwrap()
    }

    async fn state_of(app: &App, version: u32) -> (VersionState, Outcome, Option<u32>) {
        let community = app.active_community_id().unwrap();
        let root = app.paths.apps_dir();
        app.store
            .call(move |store| {
                let apps = Apps::at(store, root);
                let row = apps
                    .versions(community, "counter")
                    .unwrap()
                    .into_iter()
                    .find(|row| row.version == version)
                    .unwrap();
                let outcome = apps.outcome(community, "counter", version).unwrap();
                let current = apps.get(community, "counter").unwrap().app.current;
                Ok((row.state, outcome, current))
            })
            .await
            .unwrap()
    }

    /// One turn of bzz's loop: a tick (which asks the store at once), then
    /// polls until `wanted` is drawn, then a tick that records what the
    /// polls found.
    async fn turn(app: &mut App, wanted: &str) -> String {
        app.apps.checked = None;
        app.tick_apps().await.unwrap();
        let shown = until(app, 120, 30, wanted).await;
        app.tick_apps().await.unwrap();
        shown
    }

    fn ctrl_y() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)
    }

    async fn command(app: &mut App, arguments: &str) {
        app.app_command(arguments).await.unwrap();
    }

    async fn answer(app: &mut App, code: KeyCode) {
        app.app_review_answer_key(key(code)).await.unwrap();
    }

    /// Rule 16 end to end in bzz's window: a version runs in the pane only
    /// after the owner approved it in the overlay; an approval of a version
    /// of an open app changes it in place, with its state; a change of the
    /// declared types is shown as a change.
    #[tokio::test]
    async fn a_version_runs_only_after_its_approval_and_an_approval_changes_the_open_app_in_place()
    {
        let temporary = TempDir::new().unwrap();
        let (mut app, _channel) = managed(&temporary).await;
        let Recorded::Waiting {
            version: 1, hash, ..
        } = propose(&app, counter("v1", "\"note\"", ""), Proposer::owner()).await
        else {
            panic!("version 1 waits");
        };
        // Before the approval nothing of it runs: `:app open` refuses.
        command(&mut app, "open counter").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(
            said.contains("counter has no approved version (one waits for your answer)"),
            "{said}"
        );
        assert!(app.apps.live.is_empty());
        assert!(app.apps.open.is_empty());
        // It is announced, as an agent's command is.
        let shown = turn(&mut app, "waits for your approval").await;
        assert!(
            shown.contains(
                "counter: version 1 from you waits for your approval · Ctrl-Y to see and answer it"
            ),
            "{shown}"
        );
        // Ctrl+Y opens it: who, the hash, the types (all new), the script.
        assert!(app.app_review_key(ctrl_y()).await.unwrap());
        assert_eq!(app.presentation.overlay, Some(Overlay::AppApproval));
        let shown = screen(&mut app, 120, 40);
        for expected in [
            "┌ a version of an app waits for your answer ─",
            " a approve · r refuse ",
            "counter (Counter), version 1",
            "From:     you",
            &format!("Hash:     {hash}"),
            "Record types: note",
            "No version of this app runs yet: all of them are new.",
            "| # Counter",
            "+local app = { api = 1, types = { \"note\" } }",
            "that is the whole version",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        assert!(
            !shown.contains("waits for your approval · Ctrl-Y"),
            "the banner gives way"
        );
        // A paste only types a reason, and only while one is written.
        answer(&mut app, KeyCode::Char('a')).await;
        assert_eq!(app.presentation.overlay, None);
        let said = app.status_error.clone().unwrap_or_default();
        assert!(said.contains("counter: version 1 approved"), "{said}");
        assert_eq!(state_of(&app, 1).await.0, VersionState::Approved);

        // Approved: it opens, and runs exactly those bytes.
        command(&mut app, "open counter").await;
        let shown = turn(&mut app, "v1 count 0").await;
        assert!(shown.contains("┌ Counter ─"), "{shown}");
        assert!(shown.contains("Ctrl-c leave ┘"), "{shown}");
        for _ in 0..3 {
            assert!(app.app_key(key(KeyCode::Char('j'))));
        }
        until(&mut app, 120, 30, "v1 count 3").await;

        // One of the owner's agents proposes a version that declares one
        // type more. While it waits, the open app is the approved one.
        let Recorded::Waiting { version: 2, .. } = propose(
            &app,
            counter("v2", "\"note\", \"tag\"", ""),
            Proposer::agent("e".repeat(64), "Scout"),
        )
        .await
        else {
            panic!("version 2 waits");
        };
        let shown = turn(&mut app, "waits for your approval").await;
        assert!(shown.contains("v1 count 3"), "{shown}");
        assert!(
            shown.contains("counter: version 2 from your agent Scout"),
            "{shown}"
        );
        // Ctrl+Y reaches bzz while the app has the keyboard.
        assert_eq!(
            app.key_owner(ctrl_y()),
            crate::ui::app_pane::KeyOwner::Approval
        );
        assert!(app.app_review_key(ctrl_y()).await.unwrap());
        let shown = screen(&mut app, 120, 40);
        for expected in [
            "counter (Counter), version 2",
            "your agent Scout (eeeeeeee",
            "Record types: note, tag",
            "CHANGED from the version that runs:",
            "+ tag   new: the app could not read or write these records before",
            "Difference from version 1, which runs: 2 lines added, 2 removed:",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        answer(&mut app, KeyCode::Char('a')).await;
        // In place: the count is the one version 1 had.
        let shown = turn(&mut app, "v2 count 3").await;
        assert!(!shown.contains("waits for your approval"), "{shown}");
        let said = app.status_error.clone().unwrap_or_default();
        assert!(
            said.contains("counter: version 2 runs in place of 1, its state kept"),
            "{said}"
        );
        assert_eq!(state_of(&app, 2).await.1, Outcome::Applied);
    }

    /// M3 left this: approved bytes that fail as the pane puts them to run.
    /// A version that loads apart and then fails in the open app (its
    /// `app.reload` raises) leaves the one before running with its state,
    /// and is recorded as failed; the proposer is told the error.
    #[tokio::test]
    async fn a_version_that_fails_as_it_is_put_to_run_leaves_the_one_before_running() {
        let temporary = TempDir::new().unwrap();
        let (mut app, _channel) = managed(&temporary).await;
        let Recorded::Waiting { hash, .. } =
            propose(&app, counter("v1", "\"note\"", ""), Proposer::owner()).await
        else {
            panic!("waits");
        };
        let community = app.active_community_id().unwrap();
        let root = app.paths.apps_dir();
        app.store
            .call(move |store| {
                Apps::at(store, root)
                    .approve(community, "counter", 1, &hash)
                    .unwrap();
                Ok(())
            })
            .await
            .unwrap();
        command(&mut app, "open counter").await;
        turn(&mut app, "v1 count 0").await;
        for _ in 0..2 {
            app.app_key(key(KeyCode::Char('j')));
        }
        until(&mut app, 120, 30, "v1 count 2").await;
        let broken = counter(
            "v2",
            "\"note\"",
            "function app.reload(s, ctx) error(\"v2 cannot take this state\") end",
        );
        propose(&app, broken, Proposer::agent("e".repeat(64), "Scout")).await;
        turn(&mut app, "waits for your approval").await;
        app.open_app_review(Some("counter")).await.unwrap();
        // A yes is taken only once all of it has been on screen.
        answer(&mut app, KeyCode::Char('a')).await;
        assert!(
            app.apps
                .review
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("scroll to the end first")),
            "{:?}",
            app.apps.review.notice
        );
        assert_eq!(state_of(&app, 2).await.0, VersionState::Waiting);
        screen(&mut app, 120, 40);
        answer(&mut app, KeyCode::Char('a')).await;
        // The pane tried it, and gave the approved bytes of version 1 back.
        app.apps.checked = None;
        app.tick_apps().await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.poll_apps();
            app.tick_apps().await.unwrap();
            if state_of(&app, 2).await.0 == VersionState::Failed {
                break;
            }
            assert!(Instant::now() < deadline, "version 2 is recorded as failed");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let shown = until(&mut app, 120, 30, "v1 count 2").await;
        assert!(!shown.contains("v2 count"), "{shown}");
        let said = app.status_error.clone().unwrap_or_default();
        assert!(
            said.contains("counter: version 2 failed to run; version 1 goes on")
                && said.contains("v2 cannot take this state"),
            "{said}"
        );
        let (_, outcome, current) = state_of(&app, 2).await;
        assert_eq!(current, Some(1));
        let Outcome::Failed { error } = outcome else {
            panic!("{outcome:?}");
        };
        assert!(
            error.contains("in app.reload") && error.contains("counter.lua:"),
            "{error}"
        );
        // Still the same session: the count goes on from 2.
        app.app_key(key(KeyCode::Char('j')));
        until(&mut app, 120, 30, "v1 count 3").await;
    }

    /// Rollback, refuse, disable, enable and remove from the TUI, with the
    /// app open: the same store calls as the command line.
    #[tokio::test]
    async fn rollback_refuse_disable_enable_and_remove_from_the_tui() {
        let temporary = TempDir::new().unwrap();
        let (mut app, channel) = managed(&temporary).await;
        for (number, version) in [(1, "v1"), (2, "v2")] {
            let Recorded::Waiting { hash, .. } =
                propose(&app, counter(version, "\"note\"", ""), Proposer::owner()).await
            else {
                panic!("waits");
            };
            let community = app.active_community_id().unwrap();
            let root = app.paths.apps_dir();
            app.store
                .call(move |store| {
                    Apps::at(store, root)
                        .approve(community, "counter", number, &hash)
                        .unwrap();
                    Ok(())
                })
                .await
                .unwrap();
        }
        command(&mut app, "open counter").await;
        turn(&mut app, "v2 count 0").await;
        app.app_key(key(KeyCode::Char('j')));
        until(&mut app, 120, 30, "v2 count 1").await;
        // Back to version 1, in place, with the state.
        command(&mut app, "rollback counter").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(said.contains("counter: back to version 1 from 2"), "{said}");
        turn(&mut app, "v1 count 1").await;
        // A version refused with a reason: nothing of it runs.
        propose(
            &app,
            counter("v3", "\"note\"", ""),
            Proposer::agent("e".repeat(64), "Scout"),
        )
        .await;
        app.apps.checked = None;
        app.tick_apps().await.unwrap();
        command(&mut app, "refuse counter not on this board").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(
            said.contains("counter: version 3 refused (\"not on this board\")"),
            "{said}"
        );
        assert_eq!(
            state_of(&app, 3).await.1,
            Outcome::Refused {
                reason: Some("not on this board".into())
            }
        );
        let shown = turn(&mut app, "v1 count 1").await;
        assert!(!shown.contains("waits for your approval"), "{shown}");
        // Disabled: it closes; enabled: it is open there again.
        command(&mut app, "disable counter").await;
        app.apps.checked = None;
        app.tick_apps().await.unwrap();
        assert!(app.shown_app().is_none());
        assert!(!screen(&mut app, 120, 30).contains("v1 count"));
        command(&mut app, "enable counter").await;
        turn(&mut app, "v1 count 0").await;
        assert_eq!(app.shown_app().map(|(at, _)| at), Some(channel));
        // Removed: it closes, and nothing of it is left to open.
        command(&mut app, "remove counter").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(
            said.contains(
                "counter uninstalled; 0 records of bzz.app.counter.* stay in the channel"
            ),
            "{said}"
        );
        app.apps.checked = None;
        app.tick_apps().await.unwrap();
        assert!(app.shown_app().is_none());
        assert!(app.apps.live.is_empty());
        command(&mut app, "open counter").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(said.contains("no app named counter is installed"), "{said}");
    }

    /// `:app dev <folder>`: the owner's folder in this channel's pane, not
    /// approved and marked so, reloaded on save with its state; a broken
    /// save leaves the old version running; `:app close` ends it and the
    /// store is not told of it.
    #[tokio::test]
    async fn app_dev_runs_the_owners_folder_here_and_reloads_it_on_save() {
        let temporary = TempDir::new().unwrap();
        let (mut app, _channel) = managed(&temporary).await;
        let folder = temporary.path().join("tasks");
        crate::apps::store::templates::find("board")
            .unwrap()
            .write("tasks", None, &folder)
            .unwrap();
        command(&mut app, &format!("dev {}", folder.display())).await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(
            said.starts_with("dev: ") && said.contains("bzz.app.tasks.*"),
            "{said}"
        );
        let shown = until(&mut app, 120, 30, "To do (0)").await;
        assert!(shown.contains("not approved"), "{shown}");
        assert!(shown.contains("Tasks · not approved · dev"), "{shown}");
        app.app_key(key(KeyCode::Char('n')));
        for character in "Buy milk".chars() {
            app.app_key(key(KeyCode::Char(character)));
        }
        app.app_key(key(KeyCode::Enter));
        until(&mut app, 120, 30, "To do (1)").await;
        // A save: the new version, with the card and the selection.
        let script = folder.join("app.lua");
        let text = std::fs::read_to_string(&script).unwrap();
        std::fs::write(&script, text.replace("\"To do\"", "\"Backlog\"")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            for live in &mut app.apps.live {
                if let crate::app::pane::live::Run::Dev { folder, .. } = &mut live.run {
                    folder.look_now();
                }
            }
            app.tick_apps().await.unwrap();
            app.poll_apps();
            if screen(&mut app, 120, 30).contains("Backlog (1)") {
                break;
            }
            assert!(Instant::now() < deadline, "{}", screen(&mut app, 120, 30));
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        // A broken save: the old version goes on, and says why.
        std::fs::write(
            &script,
            format!("{}\nreturn app(", text.replace("\"To do\"", "\"Backlog\"")),
        )
        .unwrap();
        for live in &mut app.apps.live {
            if let crate::app::pane::live::Run::Dev { folder, .. } = &mut live.run {
                folder.look_now();
            }
        }
        app.tick_apps().await.unwrap();
        let shown = until(&mut app, 120, 30, "app.lua:").await;
        assert!(shown.contains("Backlog (1)"), "{shown}");
        assert!(
            app.apps
                .live
                .iter()
                .find_map(|live| live.error())
                .is_some_and(
                    |error| error.contains("reload failed: the previous version keeps running")
                ),
            "the error says the old version goes on"
        );
        // Never remembered: the store has it open nowhere, and :app close
        // ends it.
        let community = app.active_community_id().unwrap();
        let root = app.paths.apps_dir();
        let installed = app
            .store
            .call(move |store| Ok(Apps::at(store, root).list(community).unwrap().len()))
            .await
            .unwrap();
        assert_eq!(installed, 0);
        command(&mut app, "close").await;
        assert!(app.shown_app().is_none());
        assert!(app.apps.live.is_empty());
    }

    /// M6 in bzz's window: an offer from a person of the channel is told on
    /// the status line, announced by the banner as from the channel, runs
    /// nowhere before it is approved, is reviewed with who signed this
    /// exact revision, and once approved opens in the channel it was
    /// offered in. `:apps` says who offers it while it waits.
    #[tokio::test]
    async fn an_offer_from_the_channel_runs_only_after_approval_and_opens_where_it_was_offered() {
        use crate::apps::share;
        use crate::apps::store::manifest::Manifest;
        use crate::apps::store::offers::{Arrival, Step};
        use crate::service::offers::OfferNews;
        let temporary = TempDir::new().unwrap();
        let (mut app, channel) = managed(&temporary).await;
        let community = app.active_community_id().unwrap();
        let ana = nostr::Keys::generate();
        let manifest = Manifest {
            name: "counter".into(),
            title: "Counter".into(),
            api: 1,
            types: vec!["note".into()],
        };
        let bundle = crate::apps::store::Bundle {
            script: counter("v1", "\"note\"", "").into_bytes(),
            manifest_text: manifest.to_text(),
            manifest,
            brief: BRIEF.into(),
        };
        let head = crate::protocol::artifact::parse(
            &share::create(channel, Uuid::new_v4(), &bundle)
                .unwrap()
                .sign_with_keys(&ana)
                .unwrap(),
        )
        .unwrap();
        let revision = head.head.clone();
        // What the offers' sync does with the head.
        let (root, me) = (app.paths.apps_dir(), "a".repeat(64));
        let step = app
            .store
            .call({
                let root = root.clone();
                move |store| {
                    Ok(Apps::at(store, root)
                        .offer_arrived(community, &me, Arrival::of(&head, channel, false))
                        .unwrap())
                }
            })
            .await
            .unwrap();
        let Step::Examine(examine) = step else {
            panic!("{step:?}")
        };
        let submission = examine.submission.clone();
        let examined =
            tokio::task::spawn_blocking(move || examine_with(submission, &Limits::patient()))
                .await
                .unwrap()
                .unwrap();
        let told = app
            .store
            .call(move |store| {
                Ok(Apps::at(store, root)
                    .offer_examined(*examine, examined)
                    .unwrap())
            })
            .await
            .unwrap();
        app.offer_news(OfferNews::Told { channel, told });
        let said = app.status_error.clone().unwrap_or_default();
        assert!(
            said.contains("offers counter (version 1) in this channel · Ctrl-Y to read it; nothing of it runs before you approve it"),
            "{said}"
        );

        // Announced as from the channel, and nothing of it runs.
        let shown = turn(&mut app, "waits for your approval").await;
        assert!(
            shown.contains("counter: version 1 from") && shown.contains("from the channel waits"),
            "{shown}"
        );
        command(&mut app, "open counter").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(said.contains("counter has no approved version"), "{said}");
        assert!(app.apps.live.is_empty() && app.apps.open.is_empty());
        app.open_apps_panel().await.unwrap();
        let lines = app
            .apps_panel_lines()
            .into_iter()
            .map(|(line, _)| line)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(lines.contains("1 waiting (offered by"), "{lines}");
        assert!(lines.contains("Offers lately:"), "{lines}");
        app.apps_panel_key(key(KeyCode::Esc)).await.unwrap();

        // Reviewed: who signed this exact revision, by key.
        assert!(app.app_review_key(ctrl_y()).await.unwrap());
        let shown = screen(&mut app, 200, 40);
        for expected in [
            "counter (Counter), version 1",
            &format!("a person of the channel ({})", ana.public_key().to_hex()),
            &format!("this exact revision ({}…) is signed by", &revision[..16]),
            "| # Counter",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        answer(&mut app, KeyCode::Char('a')).await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(
            said.contains("counter: version 1 approved") && said.contains("where it was offered"),
            "{said}"
        );
        // It opens in that channel, and runs those bytes.
        let shown = turn(&mut app, "v1 count 0").await;
        assert_eq!(
            app.apps.open.get(&channel).map(String::as_str),
            Some("counter")
        );
        assert!(shown.contains("Counter"), "{shown}");
    }

    /// `:app share` needs the relay and a channel; a folder in development
    /// is not shared, and in a direct message nothing is.
    #[tokio::test]
    async fn sharing_says_what_it_needs() {
        let temporary = TempDir::new().unwrap();
        let (mut app, _channel) = managed(&temporary).await;
        command(&mut app, "share").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(
            said.contains("no approved app is open in this channel"),
            "{said}"
        );
        command(&mut app, "share counter").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(said.contains("sharing an app needs the relay"), "{said}");
        command(&mut app, "unshare").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(said.contains("usage: :app unshare <name>"), "{said}");
    }

    const BOARD: &str = include_str!("../../../tests/fixtures/apps/board/app.lua");
    const BOARD_BRIEF: &str = include_str!("../../../tests/fixtures/apps/board/brief.md");
    const BOARD_PRIORITY: &str =
        include_str!("../../../tests/fixtures/apps/board-priority/app.lua");
    const BOARD_PRIORITY_BRIEF: &str =
        include_str!("../../../tests/fixtures/apps/board-priority/brief.md");

    /// Proposes a version of `name`, as `bzz app add` or `propose_app` does.
    async fn propose_named(
        app: &App,
        name: &'static str,
        script: &'static str,
        brief: &'static str,
        from: Proposer,
    ) -> Recorded {
        let community = app.active_community_id().unwrap();
        let root = app.paths.apps_dir();
        let submission = Submission::new(name, script, brief, from);
        let examined =
            tokio::task::spawn_blocking(move || examine_with(submission, &Limits::patient()))
                .await
                .unwrap()
                .unwrap();
        app.store
            .call(move |store| Ok(Apps::at(store, root).record(community, examined).unwrap()))
            .await
            .unwrap()
    }

    /// The owner's example, M5's exit gate in bzz's window: the Board is
    /// open with a card selected; one of his agents, asked by a person of
    /// the channel, proposes the Board with priority and filter; he sees
    /// who proposed it and who asked, and approves it in the overlay; the
    /// open Board changes in place, the card still selected, and its new
    /// keys work. A refusal of the next version changes nothing.
    #[tokio::test]
    async fn an_agents_board_with_priority_changes_the_open_board_in_place() {
        let temporary = TempDir::new().unwrap();
        let (mut app, channel) = managed(&temporary).await;
        {
            let memory = app.apps.memory.clone().unwrap();
            let mut store = memory.lock();
            for (order, title) in ["Alpha", "Bravo", "Charlie"].iter().enumerate() {
                store.seed(
                    "card",
                    crate::apps::session::memory::content(
                        serde_json::json!({"title": title, "column": "todo", "order": order + 1}),
                    ),
                    &Who::new("b".repeat(64), "bo"),
                );
            }
        }
        let Recorded::Waiting {
            version: 1, hash, ..
        } = propose_named(&app, "board", BOARD, BOARD_BRIEF, Proposer::owner()).await
        else {
            panic!("the Board waits");
        };
        let community = app.active_community_id().unwrap();
        let root = app.paths.apps_dir();
        app.store
            .call(move |store| {
                Apps::at(store, root)
                    .approve(community, "board", 1, &hash)
                    .unwrap();
                Ok(())
            })
            .await
            .unwrap();
        command(&mut app, "open board").await;
        turn(&mut app, "To do (3)").await;
        app.focus_app();
        app.app_key(key(KeyCode::Char('j')));
        let shown = until(&mut app, 120, 30, "> Bravo").await;
        assert!(!shown.contains("Priority:"), "{shown}");
        // What `try_app` gives an agent for the Board at 40×16, on the same
        // cards and the same key, is what the pane draws inside its border.
        let window = until(&mut app, 120, 22, "> Bravo").await;
        let lines: Vec<&str> = window.lines().collect();
        let (top, left) = lines
            .iter()
            .enumerate()
            .find_map(|(row, line)| {
                line.find("┌ Board ─")
                    .map(|byte| (row, line[..byte].chars().count()))
            })
            .unwrap_or_else(|| panic!("the pane's border:\n{window}"));
        let pane = lines[top + 1..top + 17]
            .iter()
            .map(|line| {
                line.chars()
                    .skip(left + 1)
                    .take(40)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let tried = crate::apps::dev::try_script(crate::apps::dev::Trial {
            script: BOARD.as_bytes().to_vec(),
            script_name: "board.lua".into(),
            width: 40,
            height: 16,
            steps: vec![crate::apps::session::headless::Step::parse("j").unwrap()],
            seeds: crate::apps::dev::parse_seed(
                r#"[{"type":"card","content":{"title":"Alpha","column":"todo","order":1},"by":"bo"},
                    {"type":"card","content":{"title":"Bravo","column":"todo","order":2},"by":"bo"},
                    {"type":"card","content":{"title":"Charlie","column":"todo","order":3},"by":"bo"}]"#,
            )
            .unwrap(),
        })
        .unwrap()
        .tried;
        let tried = tried
            .screen
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            pane.contains("> Bravo") && pane.contains("To do (3)") && pane.lines().count() == 16,
            "{pane}"
        );
        assert_eq!(pane, tried, "the pane:\n{window}");

        // The agent's version, as `propose_app` records it.
        let asked = Proposer::agent("e".repeat(64), "Scout")
            .in_channel(channel)
            .asked_by("c".repeat(64), "Ana");
        let Recorded::Waiting { version: 2, .. } = propose_named(
            &app,
            "board",
            BOARD_PRIORITY,
            BOARD_PRIORITY_BRIEF,
            asked.clone(),
        )
        .await
        else {
            panic!("version 2 waits");
        };
        let shown = turn(&mut app, "waits for your approval").await;
        assert!(
            shown.contains("board: version 2 from your agent Scout, asked by Ana waits"),
            "{shown}"
        );
        assert!(
            shown.contains("> Bravo"),
            "the approved Board runs: {shown}"
        );
        assert!(app.app_review_key(ctrl_y()).await.unwrap());
        let shown = screen(&mut app, 120, 40);
        for expected in [
            "board (Board), version 2",
            &format!(
                "From:     your agent Scout ({}), in channel",
                "e".repeat(64)
            ),
            &format!(
                "Asked by: Ana ({}), whose message asked the agent for it",
                "c".repeat(64)
            ),
            "Record types: card",
            "The same as the version that runs.",
            "Brief:    ",
            "CHANGED from the version that runs",
            "Difference from version 1, which runs:",
        ] {
            assert!(shown.contains(expected), "`{expected}` in:\n{shown}");
        }
        // All of it is seen before the yes.
        answer(&mut app, KeyCode::Char('G')).await;
        screen(&mut app, 120, 40);
        answer(&mut app, KeyCode::Char('a')).await;
        assert_eq!(
            app.presentation.overlay, None,
            "{:?}",
            app.apps.review.notice
        );
        // In place: the card is still selected, the filter is there, and
        // the new keys work on the selected card.
        let shown = turn(&mut app, "Priority: all").await;
        assert!(shown.contains("> Bravo"), "{shown}");
        assert!(shown.contains("To do (3)"), "{shown}");
        app.focus_app();
        app.app_key(key(KeyCode::Char('2')));
        until(&mut app, 120, 30, "> Bravo P2").await;
        assert_eq!(state_of_named(&app, "board", 2).await, Outcome::Applied);

        // A third version, refused with a reason: nothing changes.
        let Recorded::Waiting { version: 3, .. } =
            propose_named(&app, "board", BOARD, BOARD_BRIEF, asked).await
        else {
            panic!("version 3 waits");
        };
        turn(&mut app, "waits for your approval").await;
        assert!(app.app_review_key(ctrl_y()).await.unwrap());
        answer(&mut app, KeyCode::Char('r')).await;
        for character in "keep the priority".chars() {
            answer(&mut app, KeyCode::Char(character)).await;
        }
        answer(&mut app, KeyCode::Enter).await;
        let shown = turn(&mut app, "> Bravo P2").await;
        assert!(shown.contains("Priority: all"), "{shown}");
        assert_eq!(
            state_of_named(&app, "board", 3).await,
            Outcome::Refused {
                reason: Some("keep the priority".into())
            }
        );
    }

    async fn state_of_named(app: &App, name: &'static str, version: u32) -> Outcome {
        let community = app.active_community_id().unwrap();
        let root = app.paths.apps_dir();
        app.store
            .call(move |store| {
                Ok(Apps::at(store, root)
                    .outcome(community, name, version)
                    .unwrap())
            })
            .await
            .unwrap()
    }

    /// `:app brief` (plan D16): the brief of the version that runs, as the
    /// owner approved it, made into a message for the channel; without a
    /// connection nothing is sent and the status line says why.
    #[tokio::test]
    async fn app_brief_posts_the_approved_brief_and_says_what_it_needs() {
        let temporary = TempDir::new().unwrap();
        let (mut app, _channel) = managed(&temporary).await;
        command(&mut app, "brief").await;
        assert_eq!(
            app.status_error.as_deref(),
            Some("no app is open in this channel: :app brief <name> posts an app's brief")
        );
        let Recorded::Waiting { hash, .. } =
            propose_named(&app, "board", BOARD, BOARD_BRIEF, Proposer::owner()).await
        else {
            panic!("waits");
        };
        command(&mut app, "brief board").await;
        let said = app.status_error.clone().unwrap_or_default();
        assert!(said.contains("board has no approved version"), "{said}");
        let community = app.active_community_id().unwrap();
        let root = app.paths.apps_dir();
        app.store
            .call(move |store| {
                Apps::at(store, root)
                    .approve(community, "board", 1, &hash)
                    .unwrap();
                Ok(())
            })
            .await
            .unwrap();
        command(&mut app, "open board").await;
        turn(&mut app, "To do (0)").await;
        // This fixture has no relay: the brief is read, and not sent.
        command(&mut app, "brief").await;
        assert_eq!(
            app.status_error.as_deref(),
            Some("a brief is posted to the relay: unlock the identity and connect first")
        );
        let message = crate::app::manage::brief_message("board", "Board", 1, BOARD_BRIEF);
        assert!(
            message.starts_with(
                "The brief of the app Board (`board`, version 1), which is open here."
            ),
            "{message}"
        );
        assert!(message.ends_with(BOARD_BRIEF.trim_end()), "{message}");
        assert!(message.len() < crate::agents::runtime::tools::MAX_TEXT_BYTES);
    }
}

/// v0.17 F5: an app shows a person by the name bzz holds, and a newer
/// profile replaces it there too. bzz gave an open app only the names it
/// had none of, so a renamed person kept the old name.
#[tokio::test]
async fn a_renamed_profile_reaches_an_open_app() {
    const WHO: &str = r#"
local app = { api = 1, types = { "note" } }
function app.init(ctx) return {} end
function app.view(s, ctx) return ui.text { "me " .. ctx.user.name } end
return app
"#;
    fn profile(name: &str, created_at: u64) -> crate::domain::Profile {
        crate::domain::Profile {
            pubkey: "a".repeat(64),
            display_name: None,
            name: Some(name.into()),
            picture: None,
            nip05: None,
            about: None,
            event_id: format!("{created_at:064}"),
            created_at,
            verified: None,
            status: None,
            presence: None,
            in_huddle: false,
        }
    }
    // The names of bzz's profiles, as the pane's tick gives them.
    fn names(app: &mut App, channel: Uuid) {
        let profiles = app.profiles.clone();
        app.apps
            .tick(Some(channel), |id| profiles.get(id).map(|p| p.label()));
    }
    let temporary = TempDir::new().unwrap();
    let (mut app, channel) = fixture(&temporary).await;
    app.profiles.insert("a".repeat(64), profile("Watcher", 1));
    app.open_app_in_memory(channel, "who", WHO);
    names(&mut app, channel);
    until(&mut app, 120, 30, "me Watcher").await;
    app.profiles.insert("a".repeat(64), profile("Cy", 2));
    names(&mut app, channel);
    let shown = until(&mut app, 120, 30, "me Cy").await;
    assert!(!shown.contains("Watcher"), "{shown}");
}

/// The demo's Board closes on its own `q`, as in any community (the key is
/// the script's), and the demo then says how to bring it back. An app of
/// that name outside the demo closes with no such word.
#[tokio::test]
async fn the_demo_says_how_to_bring_back_a_board_that_closed_itself() {
    const QUITS: &str = r#"
local app = { api = 1, types = { "card" } }
function app.init(ctx) return {} end
function app.view(s, ctx) return ui.text { "cards" } end
function app.key(s, key, ctx) if key.name == "q" then host.quit() end end
return app
"#;
    async fn closed_by_q(demo: bool) -> (App, TempDir) {
        let temporary = TempDir::new().unwrap();
        let (mut app, channel) = fixture(&temporary).await;
        if demo {
            let who = |name: &str| crate::apps::runtime::Who::new(name.repeat(64), name);
            let people = crate::demo::board::People {
                viewer: who("a"),
                ada: who("b"),
                lin: who("c"),
                sam: who("d"),
                scout: who("e"),
            };
            let store =
                crate::apps::session::SharedStore::new(crate::apps::session::MemoryStore::new(
                    people.viewer.clone(),
                    crate::apps::runtime::SystemClock::shared(),
                ));
            app.set_demo_board(crate::demo::board::DemoBoard::new(store, people));
        }
        app.open_app_in_memory(channel, crate::demo::board::NAME, QUITS);
        app.focus_app();
        until(&mut app, 120, 30, "cards").await;
        assert!(app.app_key(key(KeyCode::Char('q'))));
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.apps.open.contains_key(&channel) {
            assert!(Instant::now() < deadline, "the app never closed");
            // What the session asked is read and settled, as the tick
            // does; the store of apps is not consulted.
            app.poll_apps();
            app.settle_apps().await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(app.presentation.focus, FocusSurface::Timeline);
        (app, temporary)
    }
    let (mut app, _kept) = closed_by_q(true).await;
    assert_eq!(app.status_error.as_deref(), Some(crate::demo::BOARD_CLOSED));
    let shown = screen(&mut app, 80, 24);
    assert!(shown.contains(crate::demo::BOARD_CLOSED), "{shown}");
    let (app, _kept) = closed_by_q(false).await;
    assert_ne!(app.status_error.as_deref(), Some(crate::demo::BOARD_CLOSED));
}
