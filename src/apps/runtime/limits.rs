//! Every limit of an app, in one place. `docs/app-api.md` publishes them; a
//! test compares its table with [`published`].
//!
//! Each number says why it is that number. Two kinds:
//!
//! - what **the runtime** holds a script to: time, memory, the size of what
//!   it hands to bzz (a screen, a request, a log line, a timer);
//! - what **the records service** holds an app to: how many records, how
//!   large, how often. The runtime only checks the size of content, which
//!   it has in hand; the service behind it (the in-memory store here, the
//!   relay service from M2) checks the rest.

use std::time::Duration;

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

/// One call of a script: `init`, `view`, a handler, or the script's own top
/// level when it is loaded.
///
/// 250 ms is a hundred times the slowest key the experiment measured (2.6 ms
/// at the 95th percentile, 200 records), so no honest handler comes near it,
/// also on a loaded machine. It is short enough that an app caught in a loop
/// is seen as stopped at once, and that giving it up (three stops of one
/// handler) costs under a second of one processor core.
pub const CALL_TIME: Duration = Duration::from_millis(250);

/// What one interpreter may allocate.
///
/// The five apps of the experiment peaked at 6.3 MiB for the whole host
/// with 200 records. 64 MiB is ten times that, holds the fullest record set
/// the limits below allow when its content is text (1000 records of 16 KiB
/// are 16 MiB), and is the cap the hostile scripts of M0 were run under.
pub const MEMORY: usize = 64 * MIB;

/// The source of a script. Compiling is outside the time budget and linear
/// in the source (1 MiB compiled in 24 ms in M0), so the size is its bound.
/// 128 KiB is also what travels as one artifact in M6. The largest app of
/// the experiment is 13 KiB.
pub const SCRIPT_BYTES: usize = 128 * KIB;

/// Elements in one returned screen: every element, line, span, table cell,
/// bar and sparkline point counts as one.
///
/// bzz lays a screen out and draws it on its own thread, so this bounds
/// what a script can make bzz's thread do: laying out and drawing are one
/// pass over the elements. 10,000 is a table of 1000 records with nine
/// cells each; drawing a screen of that size was measured (the validation
/// document has the number).
pub const SCREEN_ELEMENTS: usize = 10_000;

/// Text in one returned screen, in bytes. 128 KiB is 1000 rows with 128
/// bytes shown of each, more than a row of a wide window holds. A text
/// that is wrapped is the one thing whose drawing reads every byte: at
/// 256 KiB it took a release build 17.6 ms, a whole frame.
pub const SCREEN_TEXT_BYTES: usize = 128 * KIB;

/// How deep elements may nest. The deepest screen of the experiment's apps
/// nests six levels.
pub const SCREEN_DEPTH: usize = 32;

/// Timers waiting to fire.
pub const TIMERS: usize = 32;

/// The shortest timer: one tick of bzz's own loop. The plan gives an app no
/// timer finer than a tick.
pub const TIMER_MIN_MS: u64 = 100;

/// The longest timer: a day.
pub const TIMER_MAX_MS: u64 = 86_400_000;

/// Record requests that wait for their reply. A search sends one request
/// for each key typed; 64 is more than a person types before the first
/// reply arrives.
pub const PENDING_REQUESTS: usize = 64;

/// Record requests in any minute, of every kind. It bounds how often an
/// app can make bzz's loop turn: a script that answers every refused
/// request with another would otherwise never rest. Ten a second is more
/// than a search typed fast.
pub const REQUESTS_PER_MINUTE: u32 = 600;

/// Lines of `host.log` and `print` in any minute, and the bytes of one.
/// Past it lines are dropped and one line says how many.
pub const LOG_LINES_PER_MINUTE: u32 = 120;
pub const LOG_LINE_BYTES: usize = KIB;

/// The content of one record, as the JSON text bzz sends.
///
/// The relay takes 262,144 bytes. 16 KiB keeps the fullest set an app may
/// have (1000 records) at 16 MiB, inside the memory of the interpreter that
/// has to hold it, and is far more than a card, a vote or a request needs.
pub const CONTENT_BYTES: usize = 16 * KIB;

/// How deep the content of a record, a tag or a log value may nest.
pub const DATA_DEPTH: usize = 32;

/// A request's or a timer's `tag`, as JSON text. bzz keeps it until the
/// reply or the timer.
pub const TAG_BYTES: usize = 4 * KIB;

/// The `match` of `data.list`.
pub const MATCH_BYTES: usize = 256;

/// One pasted text. A longer paste is not given to the script.
pub const PASTE_BYTES: usize = 64 * KIB;

/// A string given to `ui.width`, `ui.truncate`, `ui.edit` or `ui.paste`.
/// Each walks its text once.
pub const TEXT_ARGUMENT_BYTES: usize = 64 * KIB;

/// Record types one app may declare. A sync filter names each.
pub const TYPES: usize = 16;

/// A record type's name: `[a-z][a-z0-9_-]{0,31}`.
pub const TYPE_NAME_BYTES: usize = 32;

/// After this many stops of the same handler in a row the app is given up.
/// One stop can be a loaded machine; three are a loop.
pub const STOPS_BEFORE_GIVING_UP: u32 = 3;

/// Events that may wait for a script that does not take them. Past it the
/// runtime says the app does not respond.
pub const EVENT_BACKLOG: usize = 1024;

/// How long past its budget a call may still be running before the app is
/// shown as not responding. No such call is known (ADR, decision 6).
pub const NOT_RESPONDING_AFTER: Duration = Duration::from_secs(2);

/// Live records of one app in one channel. The relay closes a sync at 4096
/// matched artifacts; bzz stops an app long before.
pub const LIVE_RECORDS: usize = 1000;

/// A record's title for other clients, in bytes (the relay's own rule).
pub const TITLE_BYTES: usize = 512;

/// Writes in a minute for one person, shared with that person's chat (the
/// relay's quota at the pin).
pub const WRITES_PER_MINUTE: u32 = 60;

/// The limits one runtime runs with. [`Limits::default`] is what is
/// published; a test may give a call more time on a loaded machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    pub call_time: Duration,
    pub memory: usize,
    pub script_bytes: usize,
    pub screen_elements: usize,
    pub screen_text_bytes: usize,
    pub screen_depth: usize,
    pub timers: usize,
    pub timer_min_ms: u64,
    pub timer_max_ms: u64,
    pub pending_requests: usize,
    pub requests_per_minute: u32,
    pub log_lines_per_minute: u32,
    pub log_line_bytes: usize,
    pub content_bytes: usize,
    pub data_depth: usize,
    pub tag_bytes: usize,
    pub match_bytes: usize,
    pub paste_bytes: usize,
    pub text_argument_bytes: usize,
    pub types: usize,
    pub stops_before_giving_up: u32,
    pub event_backlog: usize,
    pub not_responding_after: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            call_time: CALL_TIME,
            memory: MEMORY,
            script_bytes: SCRIPT_BYTES,
            screen_elements: SCREEN_ELEMENTS,
            screen_text_bytes: SCREEN_TEXT_BYTES,
            screen_depth: SCREEN_DEPTH,
            timers: TIMERS,
            timer_min_ms: TIMER_MIN_MS,
            timer_max_ms: TIMER_MAX_MS,
            pending_requests: PENDING_REQUESTS,
            requests_per_minute: REQUESTS_PER_MINUTE,
            log_lines_per_minute: LOG_LINES_PER_MINUTE,
            log_line_bytes: LOG_LINE_BYTES,
            content_bytes: CONTENT_BYTES,
            data_depth: DATA_DEPTH,
            tag_bytes: TAG_BYTES,
            match_bytes: MATCH_BYTES,
            paste_bytes: PASTE_BYTES,
            text_argument_bytes: TEXT_ARGUMENT_BYTES,
            types: TYPES,
            stops_before_giving_up: STOPS_BEFORE_GIVING_UP,
            event_backlog: EVENT_BACKLOG,
            not_responding_after: NOT_RESPONDING_AFTER,
        }
    }
}

impl Limits {
    /// The published limits with more time for each call: for tests of what
    /// an app does, which must not fail because the machine was busy.
    pub fn patient() -> Self {
        Self {
            call_time: Duration::from_secs(20),
            ..Self::default()
        }
    }
}

fn bytes(n: usize) -> String {
    if n >= MIB && n.is_multiple_of(MIB) {
        format!("{} MiB", n / MIB)
    } else if n >= KIB && n.is_multiple_of(KIB) {
        format!("{} KiB", n / KIB)
    } else {
        format!("{n} bytes")
    }
}

/// The limits as `docs/app-api.md` lists them: a name and a value, in the
/// document's order. The test `the_document_publishes_the_limits` fails
/// when a row of the document's table says something else.
pub fn published() -> Vec<(&'static str, String)> {
    vec![
        ("Time for one call", format!("{} ms", CALL_TIME.as_millis())),
        ("Memory", bytes(MEMORY)),
        ("Script", bytes(SCRIPT_BYTES)),
        ("Elements in a screen", format!("{SCREEN_ELEMENTS}")),
        ("Text in a screen", bytes(SCREEN_TEXT_BYTES)),
        ("Nesting of a screen", format!("{SCREEN_DEPTH} levels")),
        ("Timers waiting", format!("{TIMERS}")),
        ("A timer", format!("{TIMER_MIN_MS} ms to {TIMER_MAX_MS} ms")),
        (
            "Requests waiting for a reply",
            format!("{PENDING_REQUESTS}"),
        ),
        ("Requests in a minute", format!("{REQUESTS_PER_MINUTE}")),
        (
            "Log lines in a minute",
            format!("{LOG_LINES_PER_MINUTE}, of {} each", bytes(LOG_LINE_BYTES)),
        ),
        ("A tag", bytes(TAG_BYTES)),
        ("A `match`", format!("{MATCH_BYTES} bytes")),
        ("A paste", bytes(PASTE_BYTES)),
        (
            "A string given to `ui.width`, `ui.truncate`, `ui.edit`, `ui.paste`",
            bytes(TEXT_ARGUMENT_BYTES),
        ),
        ("Record types of one app", format!("{TYPES}")),
        (
            "Content of one record",
            format!("{CONTENT_BYTES} bytes of JSON"),
        ),
        ("Nesting of content", format!("{DATA_DEPTH} levels")),
        ("Live records of one app", format!("{LIVE_RECORDS}")),
        (
            "Writes in a minute",
            format!("{WRITES_PER_MINUTE}, shared with the person's chat"),
        ),
    ]
}
