use bzz::store::models::OutboxState;
#[test]
fn outbox_states_have_stable_storage_names() {
    for (state, name) in [
        (OutboxState::Pending, "pending"),
        (OutboxState::Unknown, "unknown"),
        (OutboxState::Delivered, "delivered"),
        (OutboxState::Rejected, "rejected"),
    ] {
        assert_eq!(state.as_str(), name);
        assert_eq!(OutboxState::parse(name), Some(state));
    }
}
