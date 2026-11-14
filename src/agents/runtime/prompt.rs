//! The text a model reads for one request: who wrote, where, the message,
//! and the recent messages of the conversation. Its format is bzz's to
//! change; no harness parses it. The place and the request travel as fields
//! beside it (`docs/agent-host-protocol.md`, `session/prompt`).

use std::collections::HashMap;

use nostr::Event;

use crate::agents::runtime::admission::PlaceKind;

/// The most characters of one earlier message shown in a prompt.
const RECENT_MESSAGE_CHARS: usize = 2_000;
/// The most earlier messages shown.
pub const RECENT_MESSAGES: usize = 20;

pub struct PromptContext<'a> {
    pub agent_name: &'a str,
    pub agent_pubkey: &'a str,
    pub owner_pubkey: &'a str,
    pub channel_name: &'a str,
    pub kind: PlaceKind,
    pub request: &'a Event,
    /// Earlier messages of the conversation, oldest first.
    pub recent: &'a [Event],
    pub names: &'a HashMap<String, String>,
    /// The request was handed over before and its turn was cut short.
    pub resumed: bool,
    /// For a harness that is not bzz's own host: it never received the
    /// agent's instructions and does not know how to answer.
    pub instructions: Option<&'a str>,
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_owned()
    } else {
        let mut clipped = text.chars().take(limit).collect::<String>();
        clipped.push('…');
        clipped
    }
}

impl PromptContext<'_> {
    fn who(&self, pubkey: &str) -> String {
        if pubkey.eq_ignore_ascii_case(self.agent_pubkey) {
            return "you".into();
        }
        let name = self
            .names
            .get(pubkey)
            .cloned()
            .unwrap_or_else(|| crate::domain::abbreviated_pubkey(pubkey));
        if pubkey.eq_ignore_ascii_case(self.owner_pubkey) {
            format!("{name} (your owner)")
        } else {
            name
        }
    }

    fn place(&self) -> String {
        let name = crate::protocol::artifact::display(self.channel_name, 80);
        match self.kind {
            PlaceKind::Dm => "a direct message with your owner".into(),
            PlaceKind::Solo => format!("the Solo chat \"{name}\""),
            PlaceKind::Channel => format!("a thread in the channel #{name}"),
        }
    }
}

pub fn build(context: &PromptContext<'_>) -> String {
    let mut text = String::new();
    if context.resumed {
        text.push_str(
            "Note: your last turn on this request was cut short, so something it started may have taken effect. Before you repeat an action that changes anything, check that it did not already happen.\n\n",
        );
    }
    if let Some(instructions) = context.instructions {
        text.push_str(&format!(
            "You are {}, an agent in a Buzz team chat. Answer with the `send` tool of the `buzz` server, setting `final` to true on your answer. `read` shows earlier messages and `react` adds a reaction. What you write outside those tools is not shown to anyone.\n",
            context.agent_name
        ));
        if !instructions.trim().is_empty() {
            text.push_str(&format!("\nYour instructions:\n{}\n", instructions.trim()));
        }
        text.push('\n');
    }
    let request = context.request;
    let author = request.pubkey.to_hex();
    text.push_str(&format!(
        "From: {} ({author})\nWhere: {}\nMessage ({}):\n{}\n",
        context.who(&author),
        context.place(),
        request.id.to_hex(),
        request.content,
    ));
    if !context.recent.is_empty() {
        text.push_str("\nEarlier messages of this conversation, oldest first:\n");
        let skipped = context.recent.len().saturating_sub(RECENT_MESSAGES);
        for message in &context.recent[skipped..] {
            text.push_str(&format!(
                "- {} ({}): {}\n",
                context.who(&message.pubkey.to_hex()),
                message.id.to_hex(),
                clip(&message.content, RECENT_MESSAGE_CHARS),
            ));
        }
    }
    text
}

/// What `read` answers: the messages, oldest first, one per line.
pub fn transcript(context: &PromptContext<'_>) -> String {
    if context.recent.is_empty() {
        return "No earlier messages.".into();
    }
    let mut text = String::new();
    for message in context.recent {
        text.push_str(&format!(
            "{} {} ({}): {}\n",
            message.created_at.as_secs(),
            context.who(&message.pubkey.to_hex()),
            message.id.to_hex(),
            clip(&message.content, RECENT_MESSAGE_CHARS),
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;
    use uuid::Uuid;

    #[test]
    fn the_prompt_says_who_where_what_and_what_came_before() {
        let (owner, other, agent) = (Keys::generate(), Keys::generate(), Keys::generate());
        let channel = Uuid::new_v4();
        let message = |keys: &Keys, text: &str| {
            buzz_sdk::build_message(channel, text, None, &[], false, &[], &[])
                .unwrap()
                .sign_with_keys(keys)
                .unwrap()
        };
        let request = message(&other, "what is 17 times 23?");
        let recent = [
            message(&owner, "let us ask the agent"),
            message(&agent, "ready"),
            message(&other, &"x".repeat(RECENT_MESSAGE_CHARS + 50)),
        ];
        let names = [(other.public_key().to_hex(), "Wes".to_owned())]
            .into_iter()
            .collect();
        let (agent_hex, owner_hex) = (agent.public_key().to_hex(), owner.public_key().to_hex());
        let mut context = PromptContext {
            agent_name: "Scout",
            agent_pubkey: &agent_hex,
            owner_pubkey: &owner_hex,
            channel_name: "general",
            kind: PlaceKind::Channel,
            request: &request,
            recent: &recent,
            names: &names,
            resumed: false,
            instructions: None,
        };
        let text = build(&context);
        assert!(
            text.starts_with(&format!("From: Wes ({})\n", other.public_key().to_hex())),
            "{text}"
        );
        assert!(text.contains("Where: a thread in the channel #general\n"));
        assert!(text.contains(&format!(
            "Message ({}):\nwhat is 17 times 23?\n",
            request.id.to_hex()
        )));
        assert!(text.contains("(your owner)") && text.contains("- you ("));
        assert!(
            text.contains(&format!("{}…", "x".repeat(RECENT_MESSAGE_CHARS))),
            "long messages are clipped"
        );
        assert!(!text.contains("Buzz team chat") && !text.contains("cut short"));
        // A foreign harness gets the instructions and how to answer; a
        // resumed turn gets the caution first.
        context.instructions = Some("Be brief.");
        context.resumed = true;
        context.kind = PlaceKind::Dm;
        let text = build(&context);
        assert!(text.starts_with("Note: your last turn on this request was cut short"));
        assert!(text.contains("You are Scout") && text.contains("Your instructions:\nBe brief.\n"));
        assert!(text.contains("Where: a direct message with your owner\n"));
        let listed = transcript(&context);
        assert_eq!(listed.lines().count(), 3);
        assert!(
            listed
                .lines()
                .next()
                .unwrap()
                .contains("let us ask the agent")
        );
    }
}
