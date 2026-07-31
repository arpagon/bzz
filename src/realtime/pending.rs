use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Pending<T> {
    values: HashMap<String, T>,
}
impl<T> Pending<T> {
    pub fn insert(&mut self, id: String, value: T) -> Option<T> {
        self.values.insert(id, value)
    }
    pub fn remove(&mut self, id: &str) -> Option<T> {
        self.values.remove(id)
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
