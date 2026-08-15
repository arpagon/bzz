use std::ops::Range;

use crate::domain::DraftMention;

pub const MENTION_CAP: usize = 50;
const MAX_MENTION_LABEL_BYTES: usize = 240;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMessage {
    pub body: String,
    pub mentions: Vec<String>,
    pub attachments: Vec<crate::media::Attachment>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Composer {
    pub body: String,
    pub cursor: usize,
    pub attachments: Vec<crate::media::DraftAttachment>,
    mentions: Vec<DraftMention>,
}

impl Composer {
    pub fn insert(&mut self, character: char) {
        if self
            .mentions
            .iter()
            .any(|mention| mention.byte_end == self.cursor)
            && !mention_boundary(character)
        {
            self.mentions
                .retain(|mention| mention.byte_end != self.cursor);
        }
        self.replace_range(self.cursor..self.cursor, &character.to_string());
    }

    pub fn newline(&mut self) {
        self.insert('\n');
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((index, _)) = self.body[..self.cursor].char_indices().next_back() {
            self.replace_range(index..self.cursor, "");
        }
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.body.len() {
            return;
        }
        if let Some(character) = self.body[self.cursor..].chars().next() {
            self.replace_range(self.cursor..self.cursor + character.len_utf8(), "");
        }
    }

    pub fn move_left(&mut self) {
        if let Some((index, _)) = self.body[..self.cursor].char_indices().next_back() {
            self.cursor = index;
        }
    }

    pub fn move_right(&mut self) {
        if let Some(character) = self.body[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
    }

    pub fn active_mention(&self) -> Option<Range<usize>> {
        if !self.body.is_char_boundary(self.cursor) || in_code_region(&self.body, self.cursor) {
            return None;
        }
        let before = &self.body[..self.cursor];
        let start = before.rfind('@')?;
        let prefix = before.get(start + 1..)?;
        if prefix.len() > 80
            || prefix
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return None;
        }
        let boundary = self.body[..start].chars().next_back();
        if !boundary.is_none_or(mention_start_boundary) {
            return None;
        }
        Some(start..self.cursor)
    }

    pub fn accept_mention(&mut self, range: Range<usize>, label: &str, pubkey: &str) -> bool {
        if self.mentions.len() >= MENTION_CAP
            || !valid_pubkey(pubkey)
            || range.start > range.end
            || range.end > self.body.len()
            || !self.body.is_char_boundary(range.start)
            || !self.body.is_char_boundary(range.end)
        {
            return false;
        }
        let label = safe_label(label);
        if label.is_empty() {
            return false;
        }
        let start = range.start;
        let replacement = format!("@{label}");
        self.replace_range(range, &replacement);
        let end = start + replacement.len();
        self.mentions.push(DraftMention {
            byte_start: start,
            byte_end: end,
            pubkey: pubkey.to_ascii_lowercase(),
        });
        self.normalize_mentions();
        true
    }

    pub fn mentions(&self) -> &[DraftMention] {
        &self.mentions
    }

    pub fn set_draft(
        &mut self,
        body: String,
        attachments: Vec<crate::media::DraftAttachment>,
        mentions: Vec<DraftMention>,
    ) {
        self.body = body;
        self.cursor = self.body.len();
        self.attachments = attachments;
        self.mentions = mentions;
        self.normalize_mentions();
    }

    pub fn sendable(&self) -> bool {
        (!self.body.trim().is_empty() || !self.attachments.is_empty())
            && self
                .attachments
                .iter()
                .all(|attachment| attachment.uploaded().is_some())
    }

    pub fn take_for_send(&mut self) -> Option<String> {
        self.take_message().map(|message| message.body)
    }

    pub fn take_message(&mut self) -> Option<PreparedMessage> {
        if !self.sendable() {
            return None;
        }
        let leading = self.body.len().saturating_sub(self.body.trim_start().len());
        let body = self.body.trim().to_owned();
        let end = leading.saturating_add(body.len());
        let mentions = self
            .mentions
            .iter()
            .filter(|mention| {
                mention.byte_start >= leading
                    && mention.byte_end <= end
                    && mention.valid_for(&self.body)
            })
            .map(|mention| mention.pubkey.clone())
            .collect::<Vec<_>>();
        let attachments = std::mem::take(&mut self.attachments)
            .into_iter()
            .filter_map(|attachment| match attachment {
                crate::media::DraftAttachment::Uploaded(attachment) => Some(attachment),
                crate::media::DraftAttachment::Pending(_) => None,
            })
            .collect();
        self.body.clear();
        self.cursor = 0;
        self.mentions.clear();
        Some(PreparedMessage {
            body,
            mentions,
            attachments,
        })
    }

    fn replace_range(&mut self, range: Range<usize>, replacement: &str) {
        if range.start > range.end
            || range.end > self.body.len()
            || !self.body.is_char_boundary(range.start)
            || !self.body.is_char_boundary(range.end)
        {
            return;
        }
        let removed = range.end.saturating_sub(range.start);
        let added = replacement.len();
        self.body.replace_range(range.clone(), replacement);
        self.cursor = range.start.saturating_add(added);
        self.mentions.retain_mut(|mention| {
            if mention.byte_end <= range.start {
                return true;
            }
            if mention.byte_start >= range.end {
                mention.byte_start = shift(mention.byte_start, removed, added);
                mention.byte_end = shift(mention.byte_end, removed, added);
                return true;
            }
            false
        });
        self.normalize_mentions();
    }

    fn normalize_mentions(&mut self) {
        self.mentions
            .retain(|mention| mention.valid_for(&self.body));
        self.mentions.sort_by_key(|mention| mention.byte_start);
        let mut previous_end = 0;
        self.mentions.retain(|mention| {
            let valid = mention.byte_start >= previous_end;
            if valid {
                previous_end = mention.byte_end;
            }
            valid
        });
        self.mentions.truncate(MENTION_CAP);
    }
}

fn shift(value: usize, removed: usize, added: usize) -> usize {
    value.saturating_sub(removed).saturating_add(added)
}

fn mention_start_boundary(character: char) -> bool {
    character.is_whitespace() || matches!(character, '(' | '[' | '{' | '"' | '\'')
}

fn mention_boundary(character: char) -> bool {
    character.is_whitespace() || character.is_ascii_punctuation()
}

fn in_code_region(value: &str, cursor: usize) -> bool {
    let prefix = &value[..cursor];
    let fences = prefix.match_indices("```").count();
    if fences % 2 == 1 {
        return true;
    }
    let current_line = prefix.rsplit_once('\n').map_or(prefix, |(_, line)| line);
    current_line.matches('`').count() % 2 == 1
}

fn safe_label(label: &str) -> String {
    label
        .chars()
        .filter(|character| !character.is_control())
        .take_while(|_| true)
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_MENTION_LABEL_BYTES)
        .collect()
}

pub fn valid_pubkey(pubkey: &str) -> bool {
    pubkey.len() == 64 && pubkey.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn accepted_multibyte_mention_survives_punctuation_but_not_word_edits() {
        let mut composer = Composer {
            body: "hello @ma".into(),
            cursor: "hello @ma".len(),
            ..Composer::default()
        };
        assert!(composer.accept_mention(6..9, "Máría", KEY));
        composer.insert(',');
        assert_eq!(composer.mentions().len(), 1);
        composer.move_left();
        composer.insert('x');
        assert!(composer.mentions().is_empty());
    }

    #[test]
    fn mention_draft_validation_rejects_overlap_and_bad_offsets() {
        let mut composer = Composer::default();
        composer.set_draft(
            "@One @Two".into(),
            vec![],
            vec![
                DraftMention {
                    byte_start: 0,
                    byte_end: 4,
                    pubkey: KEY.into(),
                },
                DraftMention {
                    byte_start: 3,
                    byte_end: 8,
                    pubkey: KEY.into(),
                },
                DraftMention {
                    byte_start: 1,
                    byte_end: 2,
                    pubkey: KEY.into(),
                },
            ],
        );
        assert_eq!(composer.mentions().len(), 1);
    }

    #[test]
    fn code_and_email_do_not_activate_completion() {
        for body in ["mail me@example", "`@code", "```\n@code"] {
            let composer = Composer {
                cursor: body.len(),
                body: body.into(),
                ..Composer::default()
            };
            assert!(composer.active_mention().is_none(), "{body}");
        }
    }
}
