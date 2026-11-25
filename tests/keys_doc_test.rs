//! `docs/keys.md` is generated from the registry (v0.17 C8, plan rule 23):
//! this fails when the file in the repository is not what bzz writes.

#[test]
fn docs_keys_md_is_fresh() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/keys.md");
    let written = std::fs::read_to_string(path).unwrap_or_default();
    let generated = bzz::ui::help::keys_markdown();
    assert!(
        written == generated,
        "docs/keys.md is stale: run `{}`",
        bzz::ui::help::KEYS_DOC_COMMAND
    );
}
