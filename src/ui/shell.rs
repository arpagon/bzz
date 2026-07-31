#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pane {
    Communities,
    Channels,
    Timeline,
    Thread,
}

impl Pane {
    pub const fn next(self) -> Self {
        match self {
            Self::Communities => Self::Channels,
            Self::Channels => Self::Timeline,
            Self::Timeline => Self::Thread,
            Self::Thread => Self::Communities,
        }
    }
}
