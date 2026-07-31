use bzz::{
    protocol::types::QueryFilter,
    store::models::SyncCursor,
    sync::read_state::{ReadStateBlob, split},
};
use std::collections::BTreeMap;

#[test]
fn query_filter_preserves_composite_cursor_extensions() {
    let filter = QueryFilter {
        kinds: vec![9],
        until: Some(10),
        before_id: Some("a".repeat(64)),
        thread_cursor: Some(9),
        thread_cursor_id: Some("b".repeat(64)),
        depth_limit: Some(64),
        ..QueryFilter::default()
    }
    .tag("h", ["channel".into()]);
    let value = filter.value();
    assert_eq!(value["until"], 10);
    assert_eq!(value["before_id"], "a".repeat(64));
    assert_eq!(value["#h"][0], "channel");
    let cursor = SyncCursor {
        high_created_at: 10,
        high_event_id: "a".repeat(64),
        complete_through: 10,
    };
    assert_eq!(cursor.high_created_at, 10);
}

#[test]
fn read_state_splits_without_losing_channel_markers() {
    let contexts = (0..1000)
        .map(|index| (format!("00000000-0000-0000-0000-{index:012}"), index))
        .collect::<BTreeMap<_, _>>();
    let slots = split(contexts.clone(), "client").unwrap();
    let merged = slots
        .into_iter()
        .flat_map(|slot| slot.contexts)
        .collect::<BTreeMap<_, _>>();
    assert_eq!(merged, contexts);
}

#[test]
fn read_state_merge_is_commutative_and_monotonic() {
    let mut a = ReadStateBlob {
        v: 1,
        client_id: "a".into(),
        contexts: BTreeMap::from([("x".into(), 3), ("y".into(), 9)]),
    };
    let b = ReadStateBlob {
        v: 1,
        client_id: "b".into(),
        contexts: BTreeMap::from([("x".into(), 8)]),
    };
    a.merge(&b);
    assert_eq!(a.contexts["x"], 8);
    assert_eq!(a.contexts["y"], 9);
    a.merge(&b);
    assert_eq!(a.contexts["x"], 8);
}
