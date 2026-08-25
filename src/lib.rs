#![forbid(unsafe_code)]

pub mod agents;
pub mod app;
pub mod auth;
pub mod config;
pub mod diagnostics;
pub mod domain;
pub mod error;
pub mod media;
pub mod paths;
pub mod protocol;
pub mod realtime;
pub mod render;
pub mod service;
pub mod store;
pub mod sync;
pub mod telemetry;
pub mod ui;

pub use error::{Error, Result};
