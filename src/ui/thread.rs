use crate::domain::Message;

pub fn replies<'a>(messages: &'a [Message], root: &str) -> Vec<&'a Message> {
    let mut values = messages
        .iter()
        .filter(|message| message.root_event_id.as_deref() == Some(root))
        .collect::<Vec<_>>();
    values.sort_by_key(|message| (message.created_at, message.event_id.as_str()));
    values
}
