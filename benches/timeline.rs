use bzz::{domain::Message, render::sanitize, ui::timeline::TimelineState};
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use uuid::Uuid;

fn bench_timeline(c: &mut Criterion) {
    let messages = (0..10_000)
        .map(|index| Message {
            event_id: format!("{index:064x}"),
            channel_id: Uuid::nil(),
            pubkey: "a".repeat(64),
            created_at: index,
            content: format!("message **{index}** with unicode 🐝"),
            attachments: Vec::new(),
            root_event_id: None,
            parent_event_id: None,
            deleted: false,
            pending: false,
            rejected: None,
        })
        .collect::<Vec<_>>();
    c.bench_function("timeline cursor move in 10k messages", |bench| {
        bench.iter(|| {
            let mut state = TimelineState {
                selected_event: Some(format!("{:064x}", 5_000)),
                at_live_bottom: false,
                newer: 0,
            };
            state.move_by(black_box(&messages), 1);
            black_box(state);
        })
    });
    c.bench_function("sanitize 64KiB", |bench| {
        let text = "safe text 🐝\n".repeat(5_000);
        bench.iter(|| black_box(sanitize::text(black_box(&text))))
    });
}
criterion_group!(benches, bench_timeline);
criterion_main!(benches);
