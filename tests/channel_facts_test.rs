//! v0.13 M1: NIP-CL channel identity, Buzz channel settings and NIP-29 roles
//! decide which channels are listed and which are read-only.
use bzz::{
    config::{Config, IdentityConfig, KeyBackend},
    domain::{ChannelKind, JoinRole, MemberRole, ReadOnlyReason},
    store::Store,
};
use nostr::{EventBuilder, Keys, Kind, Tag};
use uuid::Uuid;

const ME: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fixture() -> (Store, Uuid, Keys) {
    let mut store = Store::open_memory().unwrap();
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "me".into(),
        pubkey: ME.into(),
        backend: KeyBackend::EncryptedFile,
        key_ref: "identity:test".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community("one".into(), "wss://one.example".into(), identity.id, false)
        .unwrap();
    store.sync_config(&config).unwrap();
    let relay = Keys::generate();
    store
        .pin_relay_pubkey(community, &relay.public_key().to_hex())
        .unwrap();
    (store, community, relay)
}

fn signed(relay: &Keys, kind: u16, tags: &[&[&str]], at: u64) -> nostr::Event {
    EventBuilder::new(Kind::Custom(kind), "")
        .tags(
            tags.iter()
                .map(|tag| Tag::parse(tag.iter().copied()).unwrap()),
        )
        .custom_created_at(nostr::Timestamp::from(at))
        .sign_with_keys(relay)
        .unwrap()
}

fn metadata(relay: &Keys, channel: Uuid, extra: &[&[&str]], at: u64) -> nostr::Event {
    let id = channel.to_string();
    let d = ["d", id.as_str()];
    let name = ["name", "general"];
    let mut tags: Vec<&[&str]> = vec![&d, &name];
    tags.extend_from_slice(extra);
    signed(relay, 39_000, &tags, at)
}

fn roster(relay: &Keys, channel: Uuid, role: &str) -> nostr::Event {
    let id = channel.to_string();
    signed(relay, 39_002, &[&["d", &id], &["p", ME, "", role]], 1_000)
}

fn channel_of(store: &Store, community: Uuid, channel: Uuid) -> bzz::domain::Channel {
    store
        .channels(community)
        .unwrap()
        .into_iter()
        .find(|value| value.id == channel)
        .expect("channel projected")
}

#[test]
fn nip_cl_identity_and_settings_are_projected_and_bounded() {
    let (mut store, community, relay) = fixture();
    let channel = Uuid::new_v4();
    let creator = "b".repeat(64);
    let event = metadata(
        &relay,
        channel,
        &[
            &["public"],
            &["unlisted"],
            &["restricted"],
            &["t", "stream"],
            &["t", "agent-config"],
            &["t", "agent-config"],
            &["t", "BAD LABEL"],
            &["t", "system"],
            &["t", "team:ops"],
            &["P", &creator],
            &["created_at", "1759939200"],
            &["join_role", "guest"],
            &["auto_join"],
        ],
        1_000,
    );
    store.apply_event(community, &event).unwrap();
    let projected = channel_of(&store, community, channel);
    assert_eq!(projected.kind, ChannelKind::Stream);
    assert_eq!(projected.facts.labels, vec!["agent-config", "team:ops"]);
    assert_eq!(projected.facts.creator.as_deref(), Some(creator.as_str()));
    assert_eq!(projected.facts.created_at, Some(1_759_939_200));
    assert_eq!(projected.facts.join_role, JoinRole::Guest);
    assert!(projected.facts.unlisted);
    assert!(projected.facts.auto_join);
    assert!(!projected.facts.archived);
    assert_eq!(projected.facts.viewer_role, None);
    // Not a member of an open announce channel: unlisted and read-only.
    assert!(!projected.listed_for_viewer());
    assert_eq!(projected.read_only_reason(), Some(ReadOnlyReason::Guest));
}

#[test]
fn viewer_role_decides_write_access_in_announce_channels() {
    let (mut store, community, relay) = fixture();
    let channel = Uuid::new_v4();
    store
        .apply_event(
            community,
            &metadata(
                &relay,
                channel,
                &[&["public"], &["t", "stream"], &["join_role", "guest"]],
                1_000,
            ),
        )
        .unwrap();
    store
        .apply_event(community, &roster(&relay, channel, "guest"))
        .unwrap();
    let guest = channel_of(&store, community, channel);
    assert!(guest.is_member);
    assert!(guest.listed_for_viewer());
    assert_eq!(guest.facts.viewer_role, Some(MemberRole::Guest));
    assert_eq!(guest.read_only_reason(), Some(ReadOnlyReason::Guest));

    let promoted = signed(
        &relay,
        39_002,
        &[&["d", &channel.to_string()], &["p", ME, "", "member"]],
        1_001,
    );
    store.apply_event(community, &promoted).unwrap();
    let member = channel_of(&store, community, channel);
    assert_eq!(member.facts.viewer_role, Some(MemberRole::Member));
    assert_eq!(member.read_only_reason(), None);
}

#[test]
fn every_writing_role_writes_and_unknown_or_overlong_roles_fail_closed() {
    for (role, writable) in [
        ("owner", true),
        ("admin", true),
        ("member", true),
        ("bot", true),
        ("", true),
        ("guest", false),
        ("future-role", false),
    ] {
        let (mut store, community, relay) = fixture();
        let channel = Uuid::new_v4();
        store
            .apply_event(
                community,
                &metadata(&relay, channel, &[&["t", "stream"]], 1_000),
            )
            .unwrap();
        store
            .apply_event(community, &roster(&relay, channel, role))
            .unwrap();
        let projected = channel_of(&store, community, channel);
        assert_eq!(
            projected.read_only_reason().is_none(),
            writable,
            "role {role:?}"
        );
    }
    let (mut store, community, relay) = fixture();
    let channel = Uuid::new_v4();
    store
        .apply_event(
            community,
            &metadata(&relay, channel, &[&["t", "stream"]], 1_000),
        )
        .unwrap();
    let overlong = signed(
        &relay,
        39_002,
        &[
            &["d", &channel.to_string()],
            &["p", ME, "", "owner", "extra"],
        ],
        1_000,
    );
    store.apply_event(community, &overlong).unwrap();
    assert_eq!(
        channel_of(&store, community, channel).read_only_reason(),
        Some(ReadOnlyReason::UnknownRole)
    );
}

#[test]
fn system_unknown_and_archived_channels_are_read_only_but_workflow_writes() {
    for (kind_tag, extra, expected) in [
        ("system", None, Some(ReadOnlyReason::SystemChannel)),
        ("future-type", None, Some(ReadOnlyReason::UnknownType)),
        ("workflow", None, None),
        ("forum", None, None),
        (
            "stream",
            Some(["archived", "true"]),
            Some(ReadOnlyReason::Archived),
        ),
    ] {
        let (mut store, community, relay) = fixture();
        let channel = Uuid::new_v4();
        let private = ["private"];
        let kind = ["t", kind_tag];
        let mut tags: Vec<&[&str]> = vec![&private, &kind];
        if let Some(extra) = extra.as_ref() {
            tags.push(extra);
        }
        store
            .apply_event(community, &metadata(&relay, channel, &tags, 1_000))
            .unwrap();
        store
            .apply_event(community, &roster(&relay, channel, "owner"))
            .unwrap();
        let projected = channel_of(&store, community, channel);
        assert_eq!(projected.read_only_reason(), expected, "type {kind_tag}");
        assert_eq!(projected.kind.as_str(), kind_tag);
    }
}

#[test]
fn newer_metadata_replaces_facts_and_older_is_ignored() {
    let (mut store, community, relay) = fixture();
    let channel = Uuid::new_v4();
    store
        .apply_event(
            community,
            &metadata(
                &relay,
                channel,
                &[&["t", "stream"], &["t", "first"], &["unlisted"]],
                2_000,
            ),
        )
        .unwrap();
    store
        .apply_event(
            community,
            &metadata(&relay, channel, &[&["t", "stream"], &["t", "stale"]], 1_000),
        )
        .unwrap();
    let projected = channel_of(&store, community, channel);
    assert_eq!(projected.facts.labels, vec!["first"]);
    assert!(projected.facts.unlisted);
    store
        .apply_event(
            community,
            &metadata(&relay, channel, &[&["t", "stream"]], 3_000),
        )
        .unwrap();
    let projected = channel_of(&store, community, channel);
    assert!(projected.facts.labels.is_empty());
    assert!(!projected.facts.unlisted);
}

#[test]
fn verified_snapshot_owns_viewer_membership_and_rosters_cannot_override_it() {
    use bzz::protocol::membership_snapshot::{Membership, VerifiedSnapshot};
    let (mut store, community, relay) = fixture();
    let channel = Uuid::new_v4();
    store
        .apply_event(
            community,
            &metadata(&relay, channel, &[&["public"], &["t", "stream"]], 1_000),
        )
        .unwrap();
    let snapshot = VerifiedSnapshot {
        snapshot_id: "f".repeat(64),
        types: vec![
            "dm".into(),
            "forum".into(),
            "stream".into(),
            "system".into(),
            "workflow".into(),
        ],
        memberships: vec![Membership {
            channel_id: channel,
            role: MemberRole::Owner,
            channel_type: "stream".into(),
            open: true,
            archived: false,
        }],
    };
    store
        .apply_membership_snapshot(community, &snapshot)
        .unwrap();
    let owned = channel_of(&store, community, channel);
    assert!(owned.is_member);
    assert_eq!(owned.facts.viewer_role, Some(MemberRole::Owner));

    // A stale or partial roster that omits (or downgrades) the viewer.
    let stale = signed(
        &relay,
        39_002,
        &[&["d", &channel.to_string()], &["p", &"c".repeat(64)]],
        2_000,
    );
    store.apply_event(community, &stale).unwrap();
    let still = channel_of(&store, community, channel);
    assert!(still.is_member, "roster removed a snapshot membership");
    assert_eq!(still.facts.viewer_role, Some(MemberRole::Owner));

    // Snapshot omission removes; clearing restores roster authority.
    let empty = VerifiedSnapshot {
        memberships: vec![],
        ..snapshot
    };
    store.apply_membership_snapshot(community, &empty).unwrap();
    assert!(!channel_of(&store, community, channel).is_member);
    store.clear_membership_snapshot(community).unwrap();
    let roster = signed(
        &relay,
        39_002,
        &[&["d", &channel.to_string()], &["p", ME, "", "guest"]],
        3_000,
    );
    store.apply_event(community, &roster).unwrap();
    let fallback = channel_of(&store, community, channel);
    assert!(fallback.is_member);
    assert_eq!(fallback.facts.viewer_role, Some(MemberRole::Guest));
}
