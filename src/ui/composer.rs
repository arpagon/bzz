#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Composer {
    pub body: String,
    pub cursor: usize,
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
        !self.body.trim().is_empty()
    }

    pub fn take_for_send(&mut self) -> Option<String> {
        self.sendable().then(|| {
            let text = self.body.trim().to_owned();
            self.body.clear();
            self.cursor = 0;
            text
        })
    }
}
