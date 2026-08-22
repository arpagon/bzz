mod support;

use std::time::Duration;

use bzz::{
    auth::signer::SignerHandle,
    config::{Config, IdentityConfig, KeyBackend},
    realtime::{
        session::SessionEvent,
        supervisor::{SupervisorEvent, SupervisorHandle},
    },
    service::messages::MessageService,
    store::{
        Store,
        models::{DraftSubmission, OutboxState},
        writer::StoreHandle,
    },
};
use nostr::Keys;
use tempfile::TempDir;
use uuid::Uuid;

fn configured_store(path: &std::path::Path) -> (Store, Uuid, Uuid) {
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label: "draft-test".into(),
        pubkey: "a".repeat(64),
        backend: KeyBackend::EncryptedFile,
        key_ref: "identity:draft-test".into(),
    };
    let mut config = Config::default();
    config.identities.push(identity.clone());
    let community = config
        .add_community(
            "draft-test".into(),
            "wss://draft-test.example".into(),
            identity.id,
            false,
        )
        .unwrap();
    let mut store = Store::open(path).unwrap();
    store.sync_config(&config).unwrap();
    (store, community, Uuid::new_v4())
}

fn outbox_event(channel: Uuid) -> nostr::Event {
    buzz_sdk::build_message(channel, "event", None, &[], false, &[])
        .unwrap()
        .sign_with_keys(&Keys::generate())
        .unwrap()
}

fn save_and_mark(store: &mut Store, community: Uuid, channel: Uuid, body: &str) -> DraftSubmission {
    let revision = store.save_draft(community, channel, None, body).unwrap();
    let submission = DraftSubmission {
        community_id: community,
        channel_id: channel,
        thread_root_id: None,
        revision,
    };
    assert!(store.mark_draft_sending(&submission).unwrap());
    submission
}

async fn wait_for_authentication(supervisor: &SupervisorHandle) {
    let mut events = supervisor.subscribe_events();
    tokio::time::timeout(Duration::from_secs(5), async move {
        loop {
            if matches!(
                events.recv().await,
                Ok(SupervisorEvent::Session(SessionEvent::Authenticated))
            ) {
                return;
            }
        }
    })
    .await
    .expect("fake relay authentication timed out");
}

#[tokio::test]
async fn message_service_removes_an_acknowledged_draft_before_it_can_rehydrate() {
    let temporary = TempDir::new().unwrap();
    let (mut store, community, channel) = configured_store(&temporary.path().join("bzz.db"));
    let submission = save_and_mark(&mut store, community, channel, "acknowledged draft");
    let handle = StoreHandle::spawn(store).unwrap();
    let relay = support::fake_relay::FakeRelay::start().await;
    let signer = SignerHandle::spawn(Keys::generate());
    let supervisor = SupervisorHandle::spawn(relay.url.clone(), signer.clone());
    wait_for_authentication(&supervisor).await;
    let service = MessageService::new(
        community,
        signer.clone(),
        handle.clone(),
        supervisor.clone(),
    );

    service
        .send_draft_with_media_mentions(channel, "acknowledged draft", &[], &[], submission)
        .await
        .unwrap();
    assert!(
        handle
            .call(move |store| store.draft_record(community, channel, None))
            .await
            .unwrap()
            .is_none()
    );

    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
}

#[tokio::test]
async fn message_service_recovers_a_rejected_draft_without_republishing() {
    let temporary = TempDir::new().unwrap();
    let (mut store, community, channel) = configured_store(&temporary.path().join("bzz.db"));
    let submission = save_and_mark(&mut store, community, channel, "rejected draft");
    let handle = StoreHandle::spawn(store).unwrap();
    let relay = support::fake_relay::FakeRelay::start_with_event_ack(false, "rejected").await;
    let signer = SignerHandle::spawn(Keys::generate());
    let supervisor = SupervisorHandle::spawn(relay.url.clone(), signer.clone());
    wait_for_authentication(&supervisor).await;
    let service = MessageService::new(
        community,
        signer.clone(),
        handle.clone(),
        supervisor.clone(),
    );

    assert!(
        service
            .send_draft_with_media_mentions(channel, "rejected draft", &[], &[], submission)
            .await
            .is_err()
    );
    let recovered = handle
        .call(move |store| store.draft_record(community, channel, None))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.body, "rejected draft");

    supervisor.shutdown().await;
    signer.lock().await;
    relay.stop();
}

#[test]
fn accepted_submission_is_not_hydrated_by_the_composer() {
    let temporary = TempDir::new().unwrap();
    let (mut store, community, channel) = configured_store(&temporary.path().join("bzz.db"));
    let submission = save_and_mark(&mut store, community, channel, "draft body");
    let event = outbox_event(channel);
    store.insert_outbox(community, &event).unwrap();
    assert!(
        store
            .bind_draft_submission(&submission, &event.id.to_hex())
            .unwrap()
    );

    // `i` uses this read: an in-flight submission must not masquerade as a draft.
    assert!(
        store
            .draft_record(community, channel, None)
            .unwrap()
            .is_none()
    );

    store
        .set_outbox_state(community, &event.id.to_hex(), OutboxState::Delivered, None)
        .unwrap();
    assert!(
        store
            .draft_record(community, channel, None)
            .unwrap()
            .is_none()
    );
    assert_eq!(store.draft(community, channel, None).unwrap(), "");
}

#[test]
fn rejected_or_uncertain_submission_becomes_editable_again() {
    for state in [OutboxState::Rejected, OutboxState::Unknown] {
        let temporary = TempDir::new().unwrap();
        let (mut store, community, channel) = configured_store(&temporary.path().join("bzz.db"));
        let submission = save_and_mark(&mut store, community, channel, "recover me");
        let event = outbox_event(channel);
        store.insert_outbox(community, &event).unwrap();
        store
            .bind_draft_submission(&submission, &event.id.to_hex())
            .unwrap();

        store
            .set_outbox_state(community, &event.id.to_hex(), state, Some("sanitized"))
            .unwrap();
        let recovered = store
            .draft_record(community, channel, None)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.body, "recover me");
        assert_eq!(recovered.revision, submission.revision);
    }
}

#[test]
fn late_delivery_cannot_delete_a_newer_edit() {
    let temporary = TempDir::new().unwrap();
    let (mut store, community, channel) = configured_store(&temporary.path().join("bzz.db"));
    let old = save_and_mark(&mut store, community, channel, "old");
    let event = outbox_event(channel);
    store.insert_outbox(community, &event).unwrap();
    store
        .bind_draft_submission(&old, &event.id.to_hex())
        .unwrap();

    // Typing after reopening an in-flight composer starts a new generation.
    let new_revision = store.save_draft(community, channel, None, "new").unwrap();
    assert_ne!(new_revision, old.revision);
    store
        .set_outbox_state(community, &event.id.to_hex(), OutboxState::Delivered, None)
        .unwrap();

    let current = store
        .draft_record(community, channel, None)
        .unwrap()
        .unwrap();
    assert_eq!(current.body, "new");
    assert_eq!(current.revision, new_revision);
}

#[test]
fn acknowledgement_is_scoped_to_the_exact_thread_draft() {
    let temporary = TempDir::new().unwrap();
    let (mut store, community, channel) = configured_store(&temporary.path().join("bzz.db"));
    let root_a = "a".repeat(64);
    let root_b = "b".repeat(64);
    let revision = store
        .save_draft(community, channel, Some(&root_a), "thread a")
        .unwrap();
    store
        .save_draft(community, channel, Some(&root_b), "thread b")
        .unwrap();
    let submission = DraftSubmission {
        community_id: community,
        channel_id: channel,
        thread_root_id: Some(root_a.clone()),
        revision,
    };
    assert!(store.mark_draft_sending(&submission).unwrap());
    let event = outbox_event(channel);
    store
        .insert_outbox_with_draft_submission(community, &event, &submission)
        .unwrap();
    store
        .set_outbox_state(community, &event.id.to_hex(), OutboxState::Delivered, None)
        .unwrap();

    assert!(
        store
            .draft_record(community, channel, Some(&root_a))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .draft_record(community, channel, Some(&root_b))
            .unwrap()
            .unwrap()
            .body,
        "thread b"
    );
}

#[test]
fn restart_recovers_an_interrupted_unbound_submission_without_republishing() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("bzz.db");
    let (mut store, community, channel) = configured_store(&path);
    let submission = save_and_mark(&mut store, community, channel, "restart-safe");
    assert!(
        store
            .draft_record(community, channel, None)
            .unwrap()
            .is_none()
    );
    drop(store);

    let store = Store::open(&path).unwrap();
    let recovered = store
        .draft_record(community, channel, None)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.body, "restart-safe");
    assert_eq!(recovered.revision, submission.revision);
    assert!(store.pending_outbox(community).unwrap().is_empty());
}
