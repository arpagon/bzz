//! Huddles: lifecycle and rosters are in `protocol::huddle`; live audio is
//! behind the `huddles-audio` feature. Builds without it are view-only.

#[cfg(feature = "huddles-audio")]
pub mod audio;

/// Whether this build can join huddle audio.
pub const AUDIO: bool = cfg!(feature = "huddles-audio");

/// Shown where audio would be offered in a build without the feature.
pub const VIEW_ONLY: &str =
    "this bzz build has no huddle audio (feature huddles-audio); huddles are view-only";
