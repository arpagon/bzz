//! The palette's cost on the UI thread (v0.17 C1, C12): one keystroke's
//! filter over 2,000 channels, 1,000 DMs, 1,000 people, 500 actions, 60
//! themes and 50 apps (4,610 rows), and one frame of 200×50 with the
//! palette open. Budget: the filter's p95 at or under 4 ms in release. A report, not a gate: `cargo bench --bench
//! palette` prints the numbers; under `cargo test` it runs each case once.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use bzz::ui::{
    actions::ActionContext,
    keymap::KeyMap,
    palette::{self, Candidate, Facts, Narrow, Palette, PaletteChannel, PalettePerson, Sources},
    state::{FocusSurface, Route},
    theme::{Theme, ThemeEntry, ThemeRegistry},
};
use nucleo_matcher::{Config, Matcher};
use ratatui::{Terminal, backend::TestBackend};
use uuid::Uuid;

const CHANNELS: usize = 2_000;
const PEOPLE: usize = 1_000;
const ACTIONS: usize = 500;
const THEMES: usize = 60;
const APPS: usize = 50;
const RUNS: usize = 50;
const QUERIES: &[&str] = &[
    "t", "thr", "gen", "delete", "status", "#eng", "@ada lin", "nord", "board",
];
const WORDS: &[&str] = &[
    "general",
    "engineering",
    "design",
    "launch",
    "support",
    "random",
    "ops",
    "release",
    "huddle",
    "board",
    "agents",
    "status",
    "theme",
    "thread",
    "message",
];
const NAMES: &[&str] = &[
    "Ada", "Lin", "Sam", "Scout", "Honey", "Grace", "Alan", "Edsger", "Barbara", "Ken",
];

fn facts() -> Facts {
    Facts {
        actions: ActionContext {
            route: Route::Workspace,
            focus: FocusSurface::Timeline,
            has_inbox_selection: false,
            inbox_has_context: false,
            inbox_can_reply: false,
            inbox_visible_count: 0,
            has_channel: true,
            has_selected_event: true,
            selected_event_is_own: false,
            selected_event_has_media: false,
            context_open: false,
            can_publish: true,
            delete_refused: None,
        },
        channel_is_dm: false,
        huddle_running: false,
        in_call: false,
        app_here: false,
        artifact_selected: false,
        communities: 1,
    }
}

/// 2,000 channels, 1,000 DMs, 1,000 people, 60 themes (the built-in ones
/// and generated names) and 50 apps; the table's own rows plus generated
/// actions up to 500.
fn sources() -> Sources {
    let mut channels = (0..CHANNELS)
        .map(|index| PaletteChannel {
            id: Uuid::from_u128(index as u128),
            name: format!(
                "{}-{}-{index}",
                WORDS[index % WORDS.len()],
                WORDS[(index / WORDS.len()) % WORDS.len()]
            ),
            dm: false,
            unread: index % 7 == 0,
            last_event_at: index as u64,
        })
        .collect::<Vec<_>>();
    channels.extend((0..PEOPLE).map(|index| PaletteChannel {
        id: Uuid::from_u128((CHANNELS + index) as u128),
        name: format!(
            "{} {} {index}",
            NAMES[index % NAMES.len()],
            NAMES[(index / NAMES.len()) % NAMES.len()]
        ),
        dm: true,
        unread: false,
        last_event_at: index as u64,
    }));
    palette::order_channels(&mut channels);
    let people = (0..PEOPLE)
        .map(|index| PalettePerson {
            pubkey: format!("{index:064x}"),
            name: format!(
                "{} {} {index}",
                NAMES[index % NAMES.len()],
                NAMES[(index / NAMES.len()) % NAMES.len()]
            ),
            agent: index % 50 == 0,
        })
        .collect();
    let mut themes = ThemeRegistry::entries().collect::<Vec<_>>();
    let extra = THEMES.saturating_sub(themes.len());
    themes.extend((0..extra).map(|index| ThemeEntry {
        id: Box::leak(format!("theme-{index}").into_boxed_str()),
        name: Box::leak(format!("{} theme {index}", WORDS[index % WORDS.len()]).into_boxed_str()),
    }));
    let apps = (0..APPS)
        .map(|index| format!("{}-{index}", WORDS[(index * 3) % WORDS.len()]))
        .collect();
    Sources {
        channels,
        people,
        apps,
        communities: Vec::new(),
        themes,
        theme: "bzz".into(),
        recent_emoji: Vec::new(),
    }
}

fn candidates(sources: &Sources) -> Vec<Candidate> {
    let mut all = palette::candidates(&KeyMap::builtin(), &facts(), sources);
    let table_actions = bzz::ui::action_table::TABLE
        .iter()
        .filter(|spec| spec.listed())
        .count();
    for index in table_actions..ACTIONS {
        all.push(Candidate::synthetic(
            &format!(
                "{} the {} number {index}",
                WORDS[index % WORDS.len()],
                WORDS[(index * 7) % WORDS.len()]
            ),
            "generated action words",
        ));
    }
    all
}

fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn main() {
    // `cargo bench` passes `--bench`; `cargo test --all-targets` does not.
    let measuring = std::env::args().any(|argument| argument == "--bench");
    let runs = if measuring { RUNS } else { 1 };
    if !measuring {
        println!("one run of each case, not a measurement: `cargo bench --bench palette` measures");
    }
    let sources = sources();
    let candidates = candidates(&sources);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut all = Vec::new();
    println!(
        "palette filter over {} rows ({CHANNELS} channels, {PEOPLE} DMs, {PEOPLE} people, {ACTIONS} actions, {THEMES} themes, {APPS} apps)",
        candidates.len()
    );
    for query in QUERIES {
        let (narrow, text): (Narrow, &str) = Narrow::split(query);
        let mut times = Vec::with_capacity(runs);
        for _ in 0..runs {
            let start = Instant::now();
            let ranked = palette::rank(
                black_box(&candidates),
                black_box(text),
                narrow,
                &mut matcher,
            );
            times.push(start.elapsed());
            black_box(ranked);
        }
        times.sort_unstable();
        println!(
            "  {query:>10}: median {:>8.3?} p95 {:>8.3?} max {:>8.3?}",
            percentile(&times, 0.5),
            percentile(&times, 0.95),
            times[times.len() - 1]
        );
        all.extend(times);
    }
    all.sort_unstable();
    let p95 = percentile(&all, 0.95);
    println!(
        "  all queries: median {:.3?} p95 {p95:.3?} max {:.3?} (budget p95 ≤ 4 ms: {})",
        percentile(&all, 0.5),
        all[all.len() - 1],
        match (measuring, p95 <= Duration::from_millis(4)) {
            (false, _) => "not measured",
            (true, true) => "within",
            (true, false) => "OVER",
        }
    );

    // Opening: the candidates built from the table and the channels.
    let mut opens = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        black_box(self::candidates(&sources));
        opens.push(start.elapsed());
    }
    opens.sort_unstable();
    println!(
        "palette open ({} candidates built): median {:.3?} p95 {:.3?}",
        candidates.len(),
        percentile(&opens, 0.5),
        percentile(&opens, 0.95)
    );

    // One frame of 200×50 with the palette open and "thr" typed.
    let mut palette = Palette::open(
        candidates,
        sources,
        &facts(),
        &[],
        (Route::Workspace, FocusSurface::Timeline),
    );
    palette.set_query("thr");
    let theme = Theme::default();
    let mut terminal = Terminal::new(TestBackend::new(200, 50)).expect("test backend");
    let mut frames = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        terminal
            .draw(|frame| palette::render(frame, frame.area(), &palette, &theme))
            .expect("draw");
        frames.push(start.elapsed());
    }
    frames.sort_unstable();
    println!(
        "frame 200x50 with the palette open: median {:.3?} p95 {:.3?} max {:.3?}",
        percentile(&frames, 0.5),
        percentile(&frames, 0.95),
        frames[frames.len() - 1]
    );
}
