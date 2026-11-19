//! Apps in the channel (feature `apps`).
//!
//! An app is one Lua 5.4 script that bzz runs inside itself, with only what
//! bzz lends it. [`runtime`] is the interpreter and its limits, the closed
//! API, the tree of elements and its drawing, and a headless driver. The
//! contract a script is written against is `docs/app-api.md`;
//! `docs/adr-v0.16-apps.md` says what a script is and may do.
//!
//! [`store`] is where installed apps are kept and the rule that nothing
//! runs unapproved: versions, proposals, approval and rollback, and the one
//! function that hands out a script to run. `docs/apps.md` is its guide.

//! [`dev`] runs a script outside bzz's window, on records kept in memory:
//! once, to see what it draws, or from the owner's own folder, reloaded on
//! every save.

pub mod dev;
pub mod runtime;
pub mod store;

#[cfg(test)]
mod widgets_test;
