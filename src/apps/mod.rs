//! Apps in the channel (feature `apps`).
//!
//! An app is one Lua 5.4 script that bzz runs inside itself, with only what
//! bzz lends it. [`runtime`] is the interpreter and its limits, the closed
//! API, the tree of elements and its drawing. [`session`] is one open app
//! in one channel: a runtime joined to its records, on the relay or in
//! memory, and what the app's pane owns. The contract a script is written
//! against is `docs/app-api.md`; `docs/adr-v0.16-apps.md` says what a
//! script is and may do.

pub mod runtime;
pub mod session;

#[cfg(test)]
mod widgets_test;
