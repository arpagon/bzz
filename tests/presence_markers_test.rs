//! M6.1: online and away people are marked in the sidebar DM rows and the
//! mention picker (the member list), never only by colour.

use std::collections::{HashMap, HashSet};

use bzz::{
    config::ChannelSort,
    domain::{Channel, ChannelKind, MentionCandidate, Visibility},
    protocol::presence::Presence,
    ui::{mention_picker, sidebar, state::ViewportState, theme::Theme},
};
use ratatui::{Terminal, backend::TestBackend};
use uuid::Uuid;

fn screen(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn dm(name: &str) -> Channel {
    Channel {
        id: Uuid::new_v4(),
        name: name.into(),
        about: String::new(),
        kind: ChannelKind::Dm,
        visibility: Visibility::Private,
        is_member: true,
        is_hidden: false,
        member_count: 2,
        last_event_at: None,
        facts: Default::default(),
    }
}

#[test]
fn sidebar_dm_rows_trail_the_other_persons_presence() {
    let channels = vec![dm("Ada"), dm("Bob"), dm("Cy")];
    let presence = HashMap::from([
        (channels[0].id, Presence::Online),
        (channels[1].id, Presence::Away),
        (channels[2].id, Presence::Offline),
    ]);
    let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
    terminal
        .draw(|frame| {
            sidebar::render(
                frame,
                frame.area(),
                &channels,
                &HashMap::new(),
                &ViewportState::default(),
                &HashSet::new(),
                ChannelSort::Smart,
                false,
                &presence,
                None,
                &Theme::default(),
                true,
            );
        })
        .unwrap();
    let text = screen(&terminal);
    assert!(text.contains("@Ada ●"), "{text}");
    assert!(text.contains("@Bob ◐"), "{text}");
    assert!(text.contains("@Cy "), "{text}");
    assert!(!text.contains("@Cy ●") && !text.contains("@Cy ◐"), "{text}");
}

#[test]
fn mention_picker_marks_present_members() {
    let candidate = |label: &str, presence| MentionCandidate {
        pubkey: "a".repeat(64),
        label: label.into(),
        is_agent: false,
        agent_eligibility: None,
        verified: false,
        presence,
    };
    let picker = mention_picker::MentionPicker::new(
        0..1,
        String::new(),
        vec![
            candidate("ada", Some(Presence::Online)),
            candidate("bob", Some(Presence::Away)),
            candidate("cy", Some(Presence::Offline)),
        ],
    );
    let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
    terminal
        .draw(|frame| mention_picker::render(frame, frame.area(), &picker, &Theme::default()))
        .unwrap();
    let text = screen(&terminal);
    assert!(text.contains("● @ada"), "{text}");
    assert!(text.contains("◐ @bob"), "{text}");
    assert!(text.contains(" @cy") && !text.contains("● @cy"), "{text}");
}
