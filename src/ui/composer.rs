use std::{collections::HashSet, ops::Range};

use unicode_width::UnicodeWidthChar;

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

    pub fn delete_previous_word(&mut self) {
        let before = &self.body[..self.cursor];
        let mut start = self.cursor;
        for (index, character) in before.char_indices().rev() {
            if !character.is_whitespace() {
                start = index + character.len_utf8();
                break;
            }
            start = index;
        }
        for (index, character) in self.body[..start].char_indices().rev() {
            if character.is_whitespace() {
                break;
            }
            start = index;
        }
        self.replace_range(start..self.cursor, "");
    }

    pub fn delete_to_line_start(&mut self) {
        let start = self.body[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index.saturating_add(1));
        self.replace_range(start..self.cursor, "");
    }

    pub fn delete_to_line_end(&mut self) {
        let end = self.body[self.cursor..]
            .find('\n')
            .map_or(self.body.len(), |offset| self.cursor.saturating_add(offset));
        self.replace_range(self.cursor..end, "");
    }

    pub fn move_word_left(&mut self) {
        let before = &self.body[..self.cursor];
        let mut target = 0;
        let mut in_word = false;
        for (index, character) in before.char_indices().rev() {
            if !in_word {
                if !character.is_whitespace() {
                    in_word = true;
                    target = index;
                }
            } else if character.is_whitespace() {
                self.cursor = index.saturating_add(character.len_utf8());
                return;
            } else {
                target = index;
            }
        }
        self.cursor = target;
    }

    pub fn move_word_right(&mut self) {
        let after = &self.body[self.cursor..];
        let mut in_current_word = after
            .chars()
            .next()
            .is_some_and(|character| !character.is_whitespace());
        for (offset, character) in after.char_indices() {
            if in_current_word {
                if character.is_whitespace() {
                    in_current_word = false;
                }
            } else if !character.is_whitespace() {
                self.cursor = self.cursor.saturating_add(offset);
                return;
            }
        }
        self.cursor = self.body.len();
    }

    pub fn move_to_line_start(&mut self) {
        self.cursor = self.body[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index.saturating_add(1));
    }

    pub fn move_to_line_end(&mut self) {
        self.cursor = self.body[self.cursor..]
            .find('\n')
            .map_or(self.body.len(), |offset| self.cursor.saturating_add(offset));
    }

    /// Moves the cursor to a visible composer cell without ever splitting UTF-8.
    /// Newlines and the renderer's fixed-width wrapping advance to the next row.
    pub fn set_cursor_from_display(
        &mut self,
        target_row: usize,
        target_column: usize,
        width: usize,
    ) {
        let width = width.max(1);
        let mut row = 0;
        let mut column = 0;
        for (index, character) in self.body.char_indices() {
            if row == target_row && column >= target_column {
                self.cursor = index;
                return;
            }
            if character == '\n' {
                if row == target_row {
                    self.cursor = index;
                    return;
                }
                row = row.saturating_add(1);
                column = 0;
            } else {
                column = column.saturating_add(character.width().unwrap_or(0));
                if column >= width {
                    row = row.saturating_add(1);
                    column = 0;
                }
            }
        }
        self.cursor = self.body.len();
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
        let mut seen_mentions = HashSet::new();
        let mentions = self
            .mentions
            .iter()
            .filter(|mention| {
                mention.byte_start >= leading
                    && mention.byte_end <= end
                    && mention.valid_for(&self.body)
            })
            .filter(|mention| seen_mentions.insert(mention.pubkey.as_str()))
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
    let mut safe = String::new();
    for character in label
        .trim()
        .chars()
        .filter(|character| !character.is_control())
    {
        if safe.len().saturating_add(character.len_utf8()) > MAX_MENTION_LABEL_BYTES {
            break;
        }
        safe.push(character);
    }
    safe
}

pub fn valid_pubkey(pubkey: &str) -> bool {
    pubkey.len() == 64
        && pubkey.bytes().all(|byte| {
            byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte.is_ascii_hexdigit())
        })
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

    #[test]
    fn mouse_cursor_placement_preserves_utf8_boundaries_and_wraps() {
        let mut composer = Composer {
            body: "aébc\ndef".into(),
            ..Composer::default()
        };
        composer.set_cursor_from_display(0, 2, 8);
        assert_eq!(composer.cursor, "aé".len());
        composer.set_cursor_from_display(1, 1, 8);
        assert_eq!(composer.cursor, "aébc\nd".len());
        composer.set_cursor_from_display(1, 0, 2);
        assert!(composer.body.is_char_boundary(composer.cursor));
        composer.set_draft("界ab".into(), vec![], vec![]);
        composer.set_cursor_from_display(0, 2, 8);
        assert_eq!(composer.cursor, "界".len());
    }

    #[test]
    fn owned_word_and_line_edits_preserve_utf8_boundaries() {
        let mut composer = Composer {
            body: "one two\n界three".into(),
            cursor: 0,
            ..Composer::default()
        };
        composer.move_word_right();
        assert_eq!(composer.cursor, "one ".len());
        composer.move_word_right();
        assert_eq!(composer.cursor, "one two\n".len());
        composer.move_word_left();
        assert_eq!(composer.cursor, "one ".len());
        composer.delete_to_line_end();
        assert_eq!(composer.body, "one \n界three");
        composer.move_word_right();
        composer.move_to_line_end();
        composer.delete_previous_word();
        assert_eq!(composer.body, "one \n");
        assert!(composer.body.is_char_boundary(composer.cursor));
        composer.move_to_line_start();
        composer.delete_to_line_start();
        assert_eq!(composer.body, "one \n");
    }

    #[test]
    fn send_deduplicates_valid_lowercase_mentions() {
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
                    byte_start: 5,
                    byte_end: 9,
                    pubkey: KEY.into(),
                },
            ],
        );
        assert_eq!(composer.take_message().unwrap().mentions, vec![KEY]);
        assert!(!valid_pubkey(&"A".repeat(64)));
    }
}
