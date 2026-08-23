# Native file picker attachments

**Date:** 2026-08-23
**Status:** Implemented; disposable desktop validation pending

## Goal

Make the simplest explicit desktop attachment flow reliable before expanding
clipboard file-list support. In a writable composer, `Ctrl-o` opens the OS file
chooser and allows up to eight local files. Selection stages files but never
sends a message. `Alt-o` retains the manual local-path fallback.

## Safety boundaries

- Opening the chooser requires an explicit composer action; bzz never polls.
- Linux uses XDG Desktop Portal directly, with no shell, zenity, or command
  fallback. Windows and macOS use their native open dialogs.
- Returned paths are transient, exact-composer-target scoped, never logged,
  displayed in status, synchronized, or persisted as source paths.
- Selection is capped at the remaining eight-attachment capacity. Every result
  still passes through the existing regular-file, non-symlink, open/revalidate,
  type, size, sanitization, staging, upload, and explicit-send boundaries.
- Cancel and unavailable outcomes contain no selected data and never retry.
- Late chooser/staging/upload completions cannot mutate another composer.

## Implementation

1. Add a small injectable `FilePicker` boundary with deterministic fake results.
2. Use pinned XDG portal and native-dialog dependencies only on their relevant
   targets.
3. Route chooser completion through the prioritized attachment lane.
4. Change `Ctrl-o` to native picker and expose the existing path prompt on
   `Alt-o`.
5. Add full picker-to-pending tests under a saturated general background queue,
   plus cancel, unavailable, capacity, and stale-target coverage.
6. Update ADR, media/help/release/security/troubleshooting/manual validation,
   dependency notices, and run all release gates and cross-platform CI.
