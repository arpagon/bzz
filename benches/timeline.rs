use bzz::{
    domain::Message,
    render::sanitize,
    ui::{
        composer::Composer,
        hit_map::{HitMap, HitTarget},
        redraw_gate::RedrawGate,
        theme::Theme,
        timeline::{self, TimelineState},
    },
};
use criterion::{Criterion, criterion_group, criterion_main};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use std::{collections::HashMap, hint::black_box};
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
            delivery: bzz::domain::DeliveryState::Delivered,
        })
        .collect::<Vec<_>>();
    c.bench_function("timeline cursor move in 10k messages", |bench| {
        bench.iter(|| {
            let mut state = TimelineState {
                selected_event: Some(format!("{:064x}", 5_000)),
                at_live_bottom: false,
                newer: 0,
                ..TimelineState::default()
            };
            state.move_by(black_box(&messages), 1);
            black_box(state);
        })
    });
    c.bench_function("sanitize 64KiB", |bench| {
        let text = "safe text 🐝\n".repeat(5_000);
        bench.iter(|| black_box(sanitize::text(black_box(&text))))
    });
    c.bench_function("composer unicode insert and cursor move", |bench| {
        bench.iter(|| {
            let mut composer = Composer::default();
            composer.body = "draft 🐝 ".repeat(2_000);
            composer.cursor = composer.body.len();
            composer.insert('x');
            for _ in 0..100 {
                composer.move_left();
            }
            black_box(composer)
        })
    });
    c.bench_function("semantic hit map build and resolve 10k", |bench| {
        bench.iter(|| {
            let mut map = HitMap::new(1);
            for index in 0..10_000_u16 {
                map.push(
                    Rect::new(0, index, 100, 1),
                    HitTarget::TimelineMessage(format!("{index:064x}")),
                );
            }
            black_box(map.hit(20, 9_999).is_some())
        })
    });
    let render_messages = messages.iter().take(500).cloned().collect::<Vec<_>>();
    let theme = Theme::default();
    c.bench_function(
        "timeline render 500 messages at 110-cell measure",
        |bench| {
            let mut terminal = Terminal::new(TestBackend::new(180, 48)).unwrap();
            bench.iter(|| {
                let mut state = TimelineState {
                    at_live_bottom: true,
                    ..TimelineState::default()
                };
                terminal
                    .draw(|frame| {
                        timeline::render_limited(
                            frame,
                            frame.area(),
                            &render_messages,
                            &HashMap::new(),
                            &HashMap::new(),
                            &mut state,
                            "benchmark",
                            &theme,
                            true,
                            None,
                            110,
                        );
                    })
                    .unwrap();
                black_box(state.content_height)
            })
        },
    );
    c.bench_function("redraw gate 1k idle ticks", |bench| {
        bench.iter(|| {
            let mut gate = RedrawGate::default();
            black_box(gate.take()); // initial frame
            for _ in 0..1_000 {
                black_box(gate.take());
            }
        })
    });
}
criterion_group!(benches, bench_timeline);
criterion_main!(benches);
