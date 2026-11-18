//! The app runtime. This commit holds the interpreter and what stops a
//! script; the runtime around it follows.

mod bounded;
pub mod limits;
mod pattern;
pub mod vm;

#[cfg(test)]
mod tests;
