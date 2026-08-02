#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Composer {
    pub body: String,
    pub cursor: usize,
    pub attachments: Vec<crate::media::DraftAttachment>,
}

impl Composer {
    pub fn insert(&mut self, character: char) {
        self.body.insert(self.cursor, character);
        self.cursor += character.len_utf8();
    }

    pub fn newline(&mut self) {
        self.insert('\n');
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((index, _)) = self.body[..self.cursor].char_indices().next_back() {
            self.body.remove(index);
            self.cursor = index;
        }
    }

    pub fn sendable(&self) -> bool {
        (!self.body.trim().is_empty() || !self.attachments.is_empty())
            && self
                .attachments
                .iter()
                .all(|attachment| attachment.uploaded().is_some())
    }

    pub fn take_for_send(&mut self) -> Option<String> {
        self.take_message().map(|(body, _)| body)
    }

    pub fn take_message(&mut self) -> Option<(String, Vec<crate::media::Attachment>)> {
        self.sendable().then(|| {
            let text = self.body.trim().to_owned();
            let attachments = std::mem::take(&mut self.attachments)
                .into_iter()
                .filter_map(|attachment| match attachment {
                    crate::media::DraftAttachment::Uploaded(attachment) => Some(attachment),
                    crate::media::DraftAttachment::Pending(_) => None,
                })
                .collect();
            self.body.clear();
            self.cursor = 0;
            (text, attachments)
        })
    }
}
