pub mod event;
pub mod journal;
pub mod report;

pub use event::{DiagnosticEvent, DiagnosticRecord, ErrorClass};
pub use journal::DiagnosticHandle;
