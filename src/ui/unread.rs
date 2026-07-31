use crate::domain::ReadState;

pub fn effective_read_at(state: &ReadState, context: &str, parent: Option<&str>) -> u32 {
    let own = state.contexts.get(context).copied().unwrap_or(0);
    let parent = parent
        .and_then(|key| state.contexts.get(key))
        .copied()
        .unwrap_or(0);
    own.max(parent)
}

pub fn has_unread(
    state: &ReadState,
    context: &str,
    parent: Option<&str>,
    event_at: u64,
    is_self: bool,
) -> bool {
    !is_self && event_at > u64::from(effective_read_at(state, context, parent))
}
