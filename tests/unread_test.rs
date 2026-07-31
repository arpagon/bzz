use bzz::{
    domain::ReadState,
    ui::unread::{effective_read_at, has_unread},
};
use std::collections::BTreeMap;
#[test]
fn hierarchy_and_self_authorship_are_respected() {
    let state = ReadState {
        client_id: "x".into(),
        contexts: BTreeMap::from([("channel".into(), 10), ("thread:r".into(), 20)]),
    };
    assert_eq!(effective_read_at(&state, "msg:m", Some("thread:r")), 20);
    assert!(has_unread(&state, "msg:m", Some("thread:r"), 21, false));
    assert!(!has_unread(&state, "msg:m", Some("thread:r"), 99, true));
}
