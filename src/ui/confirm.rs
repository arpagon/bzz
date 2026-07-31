#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Confirmation {
    pub title: String,
    pub detail: String,
    pub destructive: bool,
}
