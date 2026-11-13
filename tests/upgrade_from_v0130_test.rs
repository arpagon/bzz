//! Release acceptance 2: a real v0.13.0 data directory opened by v0.14.0.
//!
//! `scripts/upgrade-test.sh` builds bzz v0.13.0, writes a database with it
//! (`tests/upgrade/seed_v0130.rs`), records what v0.13.0 reads back, then runs
//! this test with `BZZ_UPGRADE_FIXTURE` pointing at that directory.

use std::path::PathBuf;

use nostr::JsonUtil;

use bzz::store::{Store, models::MessageSearchQuery};
use serde_json::Value;
use uuid::Uuid;

#[test]
#[ignore = "requires a v0.13.0 fixture from scripts/upgrade-test.sh"]
fn v0130_data_directory_upgrades_with_edits_applied_and_history_intact() {
    let fixture = PathBuf::from(
        std::env::var("BZZ_UPGRADE_FIXTURE")
            .expect("BZZ_UPGRADE_FIXTURE names the v0.13.0 fixture"),
    );
    let expected: Value =
        serde_json::from_slice(&std::fs::read(fixture.join("expected.json")).unwrap()).unwrap();
    // Work on a copy: the fixture stays a pristine v0.13.0 database.
    let temporary = tempfile::TempDir::new().unwrap();
    let database = temporary.path().join("bzz.db");
    std::fs::copy(fixture.join("bzz.db"), &database).unwrap();
    let before: u32 = rusqlite::Connection::open(&database)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        u64::from(before),
        expected["schema_version"].as_u64().unwrap()
    );

    let mut store = Store::open(&database).unwrap();
    store.search_integrity().unwrap();
    let community = Uuid::parse_str(expected["community"].as_str().unwrap()).unwrap();
    let stream = Uuid::parse_str(expected["stream"].as_str().unwrap()).unwrap();
    let own = expected["own"].as_str().unwrap();

    // Inbox: the same conversations with the same unread counts.
    let mut inbox = store
        .inbox_items(community, own)
        .unwrap()
        .into_iter()
        .map(|item| (item.conversation_id, u64::from(item.unread_count)))
        .collect::<Vec<_>>();
    inbox.sort();
    let mut old_inbox = expected["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["conversation"].as_str().unwrap().to_owned(),
                item["unread"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    old_inbox.sort();
    assert!(!old_inbox.is_empty());
    assert_eq!(inbox, old_inbox, "Inbox projection survives the upgrade");

    // FTS: the same hits for the same query, including the DM.
    let mut hits = store
        .search_messages(
            community,
            own,
            &MessageSearchQuery {
                fts_query: r#""upgradeneedle"*"#.into(),
                author: None,
                channel_id: None,
                since: None,
                until: None,
                limit: 20,
            },
        )
        .unwrap()
        .into_iter()
        .filter_map(|hit| hit.event_id)
        .collect::<Vec<_>>();
    hits.sort();
    let mut old_hits = expected["search_hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| hit.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    old_hits.sort();
    assert_eq!(old_hits.len(), 2);
    assert_eq!(hits, old_hits, "FTS answers the same after the upgrade");

    // History and draft. v0.13.0 stored an edit it never applied: the
    // migration indexes it, so the row now shows the edited text.
    let shown = store.messages(community, stream, 100).unwrap();
    assert_eq!(
        shown.len() as u64,
        expected["stream_messages"].as_u64().unwrap()
    );
    let plain = shown
        .iter()
        .find(|message| message.event_id == expected["plain"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        expected["plain_text_v013"].as_str().unwrap(),
        "plain history row",
        "v0.13.0 showed the stale text"
    );
    assert_eq!(plain.content, expected["edited_text"].as_str().unwrap());
    assert!(plain.edited, "the row is marked edited");
    // Search projection v2 finds the original message by its edited text.
    let edited_hits = store
        .search_messages(
            community,
            own,
            &MessageSearchQuery {
                fts_query: r#""editedneedle"*"#.into(),
                author: None,
                channel_id: None,
                since: None,
                until: None,
                limit: 20,
            },
        )
        .unwrap()
        .into_iter()
        .filter_map(|hit| hit.event_id)
        .collect::<Vec<_>>();
    assert_eq!(
        edited_hits,
        vec![expected["plain"].as_str().unwrap().to_owned()],
        "v0.13.0 found {} hit(s)",
        expected["edited_hits_v013"]
    );
    assert_eq!(
        store
            .draft(community, stream, Some(expected["root"].as_str().unwrap()))
            .unwrap(),
        expected["draft"].as_str().unwrap()
    );
    let channels = store.channels(community).unwrap();
    let general = channels
        .iter()
        .find(|channel| channel.id == stream)
        .unwrap();
    assert_eq!(general.kind, bzz::domain::ChannelKind::Stream);
    assert_eq!(general.read_only_reason(), None, "a member still writes");

    // v0.13.0 read the bare `private` tag as public. The relay re-sends the
    // channel's metadata on connect; that repairs the stored visibility.
    let secret = Uuid::parse_str(expected["secret"].as_str().unwrap()).unwrap();
    assert_eq!(
        expected["secret_visibility_v013"].as_str().unwrap(),
        "Public"
    );
    let metadata = nostr::Event::from_json(expected["secret_metadata"].as_str().unwrap()).unwrap();
    store.apply_event(community, &metadata).unwrap();
    let repaired = store
        .channels(community)
        .unwrap()
        .into_iter()
        .find(|channel| channel.id == secret)
        .unwrap();
    assert_eq!(repaired.visibility, bzz::domain::Visibility::Private);
    drop(store);

    let upgraded = rusqlite::Connection::open(&database).unwrap();
    let after: u32 = upgraded
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert!(
        after > before,
        "v0.14.0 migrations ran ({before} -> {after})"
    );
    let integrity: String = upgraded
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    // The pre-migration backup is a readable copy at the old version.
    let backup = std::fs::read_dir(temporary.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.to_string_lossy().ends_with(".bak"))
        .expect("pre-migration backup");
    let backed_up: u32 = rusqlite::Connection::open(&backup)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(backed_up, before);
}
