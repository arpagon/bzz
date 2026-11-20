//! Apps in the channel (feature `apps`).
//!
//! An app is one Lua 5.4 script that bzz runs inside itself, with only what
//! bzz lends it. [`runtime`] is the interpreter and its limits, the closed
//! API, the tree of elements and its drawing. [`session`] is one open app
//! in one channel: a runtime joined to its records, on the relay or in
//! memory, and what the app's pane owns. The contract a script is written
//! against is `docs/app-api.md`; `docs/adr-v0.16-apps.md` says what a
//! script is and may do.
//!
//! [`store`] is where installed apps are kept and the rule that nothing
//! runs unapproved: versions, proposals, approval and rollback, and the one
//! function that hands out a script to run. `docs/apps.md` is its guide.
//!
//! [`share`] is an app offered in a channel, on the wire: one artifact of
//! type `bzz.app` with its script, manifest and brief (M6). Taking an offer
//! in as a waiting version is `store::offers`.
//!
//! [`dev`] runs a script outside bzz's window, on records kept in memory:
//! once, to see what it draws, or from the owner's own folder, reloaded on
//! every save.

pub mod dev;
pub mod runtime;
pub mod session;
pub mod share;
pub mod store;

#[cfg(test)]
mod authoring_test;
#[cfg(test)]
mod widgets_test;
