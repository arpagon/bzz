//! Writes `docs/keys.md` from bzz's table of actions (v0.17 C8):
//! `cargo run --example keys_doc > docs/keys.md`. The test
//! `tests/keys_doc_test.rs` fails when the file is stale.

fn main() {
    print!("{}", bzz::ui::help::keys_markdown());
}
