use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant as StdInstant},
};

use nostr::Event;
use serde_json::Value;
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    time::Instant,
};
use url::Url;

use crate::{
    auth::signer::SignerHandle,
    diagnostics::{
        DiagnosticEvent, DiagnosticHandle, ErrorClass, RateLimitSource, RetryDurationBucket,
    },
    error::{Error, Result},
    realtime::{
        admission::{self, ClosureDisposition, PublishPriority, SubscriptionPriority},
        session::{self, Ack, SessionEvent},
    },
};

const COMMANDS: usize = 256;

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SupervisorEvent {
    Connecting,
    Session(SessionEvent),
    Terminal(String),
    Backoff(Duration),
    RateLimited { retry_after: Duration },
    RateLimitCleared,
}

enum Command {
    Subscribe {
        id: String,
        filters: Vec<Value>,
        priority: SubscriptionPriority,
    },
    Close(String),
    Publish {
        event: Event,
        priority: PublishPriority,
        response: oneshot::Sender<Result<Ack>>,
    },
    Reconnect,
    Shutdown,
}

#[derive(Clone)]
pub struct SupervisorHandle {
    commands: mpsc::Sender<Command>,
    events: broadcast::Sender<SupervisorEvent>,
    diagnostics: DiagnosticHandle,
}

impl SupervisorHandle {
    pub fn spawn(relay: Url, signer: SignerHandle) -> Self {
        Self::spawn_with_diagnostics(relay, signer, DiagnosticHandle::disabled())
    }

    pub fn spawn_with_diagnostics(
        relay: Url,
        signer: SignerHandle,
        diagnostics: DiagnosticHandle,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(COMMANDS);
        let (events, _) = broadcast::channel(2048);
        tokio::spawn(run(
            relay,
            signer,
            receiver,
            events.clone(),
            diagnostics.clone(),
        ));
        Self {
            commands,
            events,
            diagnostics,
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<SupervisorEvent> {
        self.events.subscribe()
    }

    pub fn diagnostics(&self) -> &DiagnosticHandle {
        &self.diagnostics
    }

    pub async fn subscribe(&self, id: impl Into<String>, filters: Vec<Value>) -> Result<()> {
        self.subscribe_with_priority(id, filters, SubscriptionPriority::Background)
            .await
    }

    pub async fn subscribe_with_priority(
        &self,
        id: impl Into<String>,
        filters: Vec<Value>,
        priority: SubscriptionPriority,
    ) -> Result<()> {
        self.commands
            .send(Command::Subscribe {
                id: id.into(),
                filters,
                priority,
            })
            .await
            .map_err(|_| Error::Network("supervisor stopped".into()))
    }

    pub async fn close(&self, id: impl Into<String>) -> Result<()> {
        self.commands
            .send(Command::Close(id.into()))
            .await
            .map_err(|_| Error::Network("supervisor stopped".into()))
    }

    pub async fn publish(&self, event: Event) -> Result<Ack> {
        self.publish_with_priority(event, PublishPriority::Interactive)
            .await
    }

    pub async fn publish_recovery(&self, event: Event) -> Result<Ack> {
        self.publish_with_priority(event, PublishPriority::Recovery)
            .await
    }

    pub async fn publish_maintenance(&self, event: Event) -> Result<Ack> {
        self.publish_with_priority(event, PublishPriority::Maintenance)
            .await
    }

    async fn publish_with_priority(&self, event: Event, priority: PublishPriority) -> Result<Ack> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Command::Publish {
                event,
                priority,
                response: tx,
            })
            .await
            .map_err(|_| Error::Network("supervisor stopped".into()))?;
        rx.await
            .map_err(|_| Error::Network("supervisor stopped".into()))?
    }

    pub async fn reconnect(&self) {
        self.diagnostics.emit(DiagnosticEvent::ReconnectRequested {
            source: "user".into(),
        });
        let _ = self.commands.send(Command::Reconnect).await;
    }

    pub async fn shutdown(&self) {
        let _ = self.commands.send(Command::Shutdown).await;
    }
}

#[derive(Clone, Debug)]
struct DesiredSubscription {
    filters: Vec<Value>,
    priority: SubscriptionPriority,
    needs_send: bool,
    retry_at: Option<Instant>,
    retry_attempt: u32,
}

struct PendingPublication {
    event: Event,
    queued_at: Instant,
    response: oneshot::Sender<Result<Ack>>,
}

#[derive(Default)]
struct PublicationQueues {
    interactive: VecDeque<PendingPublication>,
    recovery: VecDeque<PendingPublication>,
    maintenance: VecDeque<PendingPublication>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdmissionLane {
    InteractivePublication,
    Subscription,
    DeferredPublication,
    Idle,
}

fn admission_lane(
    publication_inflight: bool,
    has_interactive: bool,
    next_subscription: Option<SubscriptionPriority>,
    has_deferred: bool,
) -> AdmissionLane {
    if !publication_inflight && has_interactive {
        AdmissionLane::InteractivePublication
    } else if matches!(
        next_subscription,
        Some(SubscriptionPriority::Foreground | SubscriptionPriority::Baseline)
    ) {
        AdmissionLane::Subscription
    } else if !publication_inflight && has_deferred {
        AdmissionLane::DeferredPublication
    } else if next_subscription.is_some() {
        AdmissionLane::Subscription
    } else {
        AdmissionLane::Idle
    }
}

impl PublicationQueues {
    fn len(&self) -> usize {
        self.interactive.len() + self.recovery.len() + self.maintenance.len()
    }

    fn has_interactive(&self) -> bool {
        !self.interactive.is_empty()
    }

    fn has_deferred(&self) -> bool {
        !self.recovery.is_empty() || !self.maintenance.is_empty()
    }

    fn push(&mut self, priority: PublishPriority, publication: PendingPublication) {
        match priority {
            PublishPriority::Interactive => self.interactive.push_back(publication),
            PublishPriority::Recovery => self.recovery.push_back(publication),
            PublishPriority::Maintenance => self.maintenance.push_back(publication),
        }
    }

    fn pop_interactive(&mut self) -> Option<PendingPublication> {
        self.interactive.pop_front()
    }

    fn pop_deferred(&mut self) -> Option<PendingPublication> {
        self.recovery
            .pop_front()
            .or_else(|| self.maintenance.pop_front())
    }

    fn pop(&mut self) -> Option<PendingPublication> {
        self.pop_interactive().or_else(|| self.pop_deferred())
    }

    fn reject_expired(&mut self, now: Instant) {
        for queue in [
            &mut self.interactive,
            &mut self.recovery,
            &mut self.maintenance,
        ] {
            while queue.front().is_some_and(|item| {
                now.duration_since(item.queued_at) >= admission::PUBLICATION_QUEUE_TIMEOUT
            }) {
                if let Some(item) = queue.pop_front() {
                    let _ = item.response.send(Err(admission::local_admission_error(
                        "queue wait expired before wire send",
                    )));
                }
            }
        }
    }

    fn reject_all(&mut self, message: &str) {
        let reason = match message {
            "session disconnected before publication admission" => {
                "session disconnected before wire send"
            }
            "publication cancelled by reconnect" => "cancelled by reconnect before wire send",
            "publication cancelled by shutdown" => "cancelled by shutdown before wire send",
            "session stopped before publication admission" => "session stopped before wire send",
            "supervisor stopped before publication admission" => {
                "supervisor stopped before wire send"
            }
            _ => "cancelled before wire send",
        };
        while let Some(item) = self.pop() {
            let _ = item
                .response
                .send(Err(admission::local_admission_error(reason)));
        }
    }
}

struct PublishCompletion {
    rate_limit: Option<Duration>,
}

fn relay_origin(relay: &Url) -> String {
    relay.origin().ascii_serialization()
}

fn elapsed_millis(started: StdInstant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn remember_subscription(
    desired: &mut BTreeMap<String, DesiredSubscription>,
    id: String,
    filters: Vec<Value>,
    priority: SubscriptionPriority,
) {
    desired
        .entry(id)
        .and_modify(|entry| {
            entry.filters.clone_from(&filters);
            entry.priority = priority;
            entry.needs_send = true;
            entry.retry_at = None;
            entry.retry_attempt = 0;
        })
        .or_insert(DesiredSubscription {
            filters,
            priority,
            needs_send: true,
            retry_at: None,
            retry_attempt: 0,
        });
}

fn next_subscription(
    desired: &BTreeMap<String, DesiredSubscription>,
    now: Instant,
) -> Option<String> {
    desired
        .iter()
        .filter(|(_, entry)| {
            entry.needs_send && entry.retry_at.is_none_or(|retry_at| retry_at <= now)
        })
        .min_by_key(|(id, entry)| (entry.priority, id.as_str()))
        .map(|(id, _)| id.clone())
}

fn activate_rate_limit(
    deadline: &mut Option<Instant>,
    retry_after: Duration,
    source: RateLimitSource,
    events: &broadcast::Sender<SupervisorEvent>,
    diagnostics: &DiagnosticHandle,
) {
    let retry_after = retry_after
        .max(Duration::from_secs(1))
        .min(admission::MAX_RETRY_AFTER);
    let candidate = Instant::now() + retry_after;
    if deadline.is_some_and(|current| current >= candidate) {
        return;
    }
    *deadline = Some(candidate);
    diagnostics.emit_local(DiagnosticEvent::RateLimitActivated {
        source,
        retry_bucket: RetryDurationBucket::from_seconds(retry_after.as_secs()),
    });
    let _ = events.send(SupervisorEvent::RateLimited { retry_after });
}

fn normalized_publish_result(result: Result<Ack>) -> (Result<Ack>, Option<Duration>) {
    match result {
        Ok(mut ack) if !ack.accepted => {
            let rate_limit = admission::rate_limit_retry_after(&ack.message);
            if let Some(retry_after) = rate_limit {
                ack.message = admission::fixed_rate_limit_message(retry_after);
            }
            (Ok(ack), rate_limit)
        }
        Err(Error::Network(message)) => {
            let rate_limit = admission::rate_limit_retry_after(&message);
            let error = rate_limit.map_or_else(
                || Error::Network(message),
                |retry_after| Error::Network(admission::fixed_rate_limit_message(retry_after)),
            );
            // A legacy uncorrelated rejection also arrives as SessionEvent::Notice,
            // which owns gate activation and its exact hint. Avoid double-extending
            // the deadline when the pending publication resolves first.
            (Err(error), None)
        }
        other => (other, None),
    }
}

async fn run(
    relay: Url,
    signer: SignerHandle,
    mut commands: mpsc::Receiver<Command>,
    events: broadcast::Sender<SupervisorEvent>,
    diagnostics: DiagnosticHandle,
) {
    let mut desired: BTreeMap<String, DesiredSubscription> = BTreeMap::new();
    let mut publications = PublicationQueues::default();
    let publish_inflight = Arc::new(AtomicBool::new(false));
    let (publish_done_tx, mut publish_done_rx) = mpsc::channel::<PublishCompletion>(8);
    let mut rate_limit_deadline: Option<Instant> = None;
    let mut rate_limit_visible = false;
    let mut delay = Duration::from_millis(250);
    let mut attempt = 0_u32;
    let relay_origin = relay_origin(&relay);

    'outer: loop {
        while let Ok(done) = publish_done_rx.try_recv() {
            if let Some(retry_after) = done.rate_limit {
                activate_rate_limit(
                    &mut rate_limit_deadline,
                    retry_after,
                    RateLimitSource::PublishAck,
                    &events,
                    &diagnostics,
                );
                rate_limit_visible = true;
            }
        }
        attempt = attempt.saturating_add(1);
        diagnostics.emit(DiagnosticEvent::ConnectStarted {
            relay_origin: relay_origin.clone(),
            attempt,
        });
        let connect_started = StdInstant::now();
        let _ = events.send(SupervisorEvent::Connecting);
        let connected =
            session::connect_with_diagnostics(relay.clone(), signer.clone(), diagnostics.clone())
                .await;
        let (handle, mut session_events) = match connected {
            Ok(value) => {
                delay = Duration::from_millis(250);
                attempt = 0;
                value
            }
            Err(error @ Error::Access(_)) | Err(error @ Error::Auth(_)) => {
                diagnostics.emit(DiagnosticEvent::ConnectFailed {
                    phase: "auth".into(),
                    error_class: ErrorClass::from_error(&error),
                    duration_ms: elapsed_millis(connect_started),
                });
                let _ = events.send(SupervisorEvent::Terminal(error.to_string()));
                loop {
                    match commands.recv().await {
                        Some(Command::Reconnect) => break,
                        Some(Command::Subscribe {
                            id,
                            filters,
                            priority,
                        }) => remember_subscription(&mut desired, id, filters, priority),
                        Some(Command::Close(id)) => {
                            desired.remove(&id);
                        }
                        Some(Command::Publish { response, .. }) => {
                            let _ = response
                                .send(Err(Error::Access("session is not authenticated".into())));
                        }
                        Some(Command::Shutdown) | None => break 'outer,
                    }
                }
                continue;
            }
            Err(error) => {
                diagnostics.emit(DiagnosticEvent::ConnectFailed {
                    phase: "transport".into(),
                    error_class: ErrorClass::from_error(&error),
                    duration_ms: elapsed_millis(connect_started),
                });
                diagnostics.emit(DiagnosticEvent::BackoffScheduled {
                    attempt,
                    delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                });
                let _ = events.send(SupervisorEvent::Session(SessionEvent::Disconnected(
                    error.to_string(),
                )));
                let _ = events.send(SupervisorEvent::Backoff(delay));
                tokio::select! {
                    _=tokio::time::sleep(delay)=>{},
                    command=commands.recv()=>match command {
                        Some(Command::Shutdown)|None=>break 'outer,
                        Some(Command::Subscribe{id,filters,priority})=>remember_subscription(&mut desired,id,filters,priority),
                        Some(Command::Close(id))=>{desired.remove(&id);},
                        Some(Command::Publish{response,..})=>{let _=response.send(Err(Error::Network("session is offline".into())));},
                        Some(Command::Reconnect)=>{}
                    }
                }
                delay = (delay * 2).min(Duration::from_secs(20));
                continue;
            }
        };

        for entry in desired.values_mut() {
            entry.needs_send = true;
            entry.retry_at = None;
        }
        let now = Instant::now();
        if let Some(deadline) = rate_limit_deadline {
            if deadline > now {
                let _ = events.send(SupervisorEvent::RateLimited {
                    retry_after: deadline.duration_since(now),
                });
                rate_limit_visible = true;
            } else {
                rate_limit_deadline = None;
                if rate_limit_visible {
                    diagnostics.emit_local(DiagnosticEvent::RateLimitCleared);
                    let _ = events.send(SupervisorEvent::RateLimitCleared);
                    rate_limit_visible = false;
                }
            }
        }
        let mut wire_open = BTreeSet::new();
        let mut locally_closing = BTreeSet::new();
        let mut admission_tick =
            tokio::time::interval_at(Instant::now(), admission::FRAME_INTERVAL);
        admission_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                biased;
                Some(done)=publish_done_rx.recv()=>{
                    if let Some(retry_after)=done.rate_limit {
                        activate_rate_limit(
                            &mut rate_limit_deadline,
                            retry_after,
                            RateLimitSource::PublishAck,
                            &events,
                            &diagnostics,
                        );
                        rate_limit_visible = true;
                    }
                }
                event=session_events.recv()=>match event {
                    Some(event@SessionEvent::Disconnected(_))=>{
                        diagnostics.emit(DiagnosticEvent::ReconnectRequested { source:"supervisor".into() });
                        publications.reject_all("session disconnected before publication admission");
                        let _=events.send(SupervisorEvent::Session(event));
                        break;
                    }
                    Some(SessionEvent::Closed{subscription,message})=>{
                        if locally_closing.remove(&subscription) {
                            continue;
                        }
                        wire_open.remove(&subscription);
                        match admission::classify_closed(&message) {
                            ClosureDisposition::RateLimited { retry_after } => {
                                activate_rate_limit(
                                    &mut rate_limit_deadline,
                                    retry_after,
                                    RateLimitSource::Closed,
                                    &events,
                                    &diagnostics,
                                );
                                rate_limit_visible = true;
                                if let Some(entry)=desired.get_mut(&subscription) {
                                    entry.needs_send=true;
                                    entry.retry_attempt=entry.retry_attempt.saturating_add(1);
                                    let gate=rate_limit_deadline.unwrap_or_else(Instant::now);
                                    entry.retry_at=Some(gate+admission::retry_jitter(&subscription));
                                }
                            }
                            ClosureDisposition::Retryable => {
                                if let Some(entry)=desired.get_mut(&subscription) {
                                    let wait=admission::retry_backoff(entry.retry_attempt)
                                        + admission::retry_jitter(&subscription);
                                    entry.retry_attempt=entry.retry_attempt.saturating_add(1);
                                    entry.needs_send=true;
                                    entry.retry_at=Some(Instant::now()+wait);
                                }
                            }
                            terminal @ (ClosureDisposition::TerminalAccess | ClosureDisposition::TerminalProtocol) => {
                                desired.remove(&subscription);
                                let _=events.send(SupervisorEvent::Session(SessionEvent::Closed {
                                    subscription,
                                    message: admission::terminal_message(terminal).into(),
                                }));
                            }
                        }
                    }
                    Some(SessionEvent::Notice(message))=>{
                        if let Some(retry_after)=admission::rate_limit_retry_after(&message) {
                            activate_rate_limit(
                                &mut rate_limit_deadline,
                                retry_after,
                                RateLimitSource::Notice,
                                &events,
                                &diagnostics,
                            );
                            rate_limit_visible = true;
                            let gate=rate_limit_deadline.unwrap_or_else(Instant::now);
                            for (id,entry) in &mut desired {
                                if entry.needs_send {
                                    entry.retry_at=Some(gate+admission::retry_jitter(id));
                                }
                            }
                        } else {
                            let _=events.send(SupervisorEvent::Session(SessionEvent::Notice(message)));
                        }
                    }
                    Some(event@SessionEvent::Eose(_))=>{
                        let subscription=match &event {
                            SessionEvent::Eose(subscription)=>subscription.clone(),
                            _=>unreachable!(),
                        };
                        if let Some(entry)=desired.get_mut(&subscription) {
                            entry.retry_attempt=0;
                            entry.retry_at=None;
                        }
                        wire_open.insert(subscription);
                        let _=events.send(SupervisorEvent::Session(event));
                    }
                    Some(event@SessionEvent::Event{..})=>{
                        let subscription=match &event {
                            SessionEvent::Event{subscription,..}=>subscription.clone(),
                            _=>unreachable!(),
                        };
                        if let Some(entry)=desired.get_mut(&subscription) {
                            entry.retry_attempt=0;
                            entry.retry_at=None;
                        }
                        wire_open.insert(subscription);
                        let _=events.send(SupervisorEvent::Session(event));
                    }
                    Some(event)=>{let _=events.send(SupervisorEvent::Session(event));}
                    None=>{
                        publications.reject_all("session stopped before publication admission");
                        break;
                    },
                },
                command=commands.recv()=>match command {
                    Some(Command::Subscribe{id,filters,priority})=>{
                        remember_subscription(&mut desired,id,filters,priority);
                    }
                    Some(Command::Close(id))=>{
                        desired.remove(&id);
                        if wire_open.remove(&id) {
                            locally_closing.insert(id.clone());
                            if handle.close(id).await.is_err(){break;}
                        }
                    }
                    Some(Command::Publish{event,priority,response})=>{
                        if publications.len()>=admission::MAX_PENDING_PUBLICATIONS {
                            let _=response.send(Err(admission::local_admission_error("queue is full before wire send")));
                        } else {
                            publications.push(priority,PendingPublication {
                                event,
                                queued_at:Instant::now(),
                                response,
                            });
                        }
                    }
                    Some(Command::Reconnect)=>{
                        publications.reject_all("publication cancelled by reconnect");
                        handle.shutdown().await;
                        break;
                    }
                    Some(Command::Shutdown)=>{
                        publications.reject_all("publication cancelled by shutdown");
                        for id in wire_open { let _=handle.close(id).await; }
                        handle.shutdown().await;
                        break 'outer;
                    }
                    None=>{
                        publications.reject_all("supervisor stopped before publication admission");
                        handle.shutdown().await;
                        break 'outer;
                    }
                },
                _=admission_tick.tick()=>{
                    let now=Instant::now();
                    publications.reject_expired(now);
                    if rate_limit_deadline.is_some_and(|deadline| deadline<=now) {
                        rate_limit_deadline=None;
                        if rate_limit_visible {
                            diagnostics.emit_local(DiagnosticEvent::RateLimitCleared);
                            let _=events.send(SupervisorEvent::RateLimitCleared);
                            rate_limit_visible=false;
                        }
                    }
                    if rate_limit_deadline.is_some() {
                        continue;
                    }
                    let next_subscription=next_subscription(&desired,now);
                    let next_priority=next_subscription
                        .as_ref()
                        .and_then(|id|desired.get(id))
                        .map(|entry|entry.priority);
                    let lane=admission_lane(
                        publish_inflight.load(Ordering::SeqCst),
                        publications.has_interactive(),
                        next_priority,
                        publications.has_deferred(),
                    );
                    let publication=match lane {
                        AdmissionLane::InteractivePublication=>publications.pop_interactive(),
                        AdmissionLane::DeferredPublication=>publications.pop_deferred(),
                        AdmissionLane::Subscription|AdmissionLane::Idle=>None,
                    };
                    if let Some(publication)=publication {
                        publish_inflight.store(true,Ordering::SeqCst);
                        let current=handle.clone();
                        let inflight=publish_inflight.clone();
                        let done=publish_done_tx.clone();
                        tokio::spawn(async move {
                            let (result,rate_limit)=normalized_publish_result(
                                current.publish(publication.event).await
                            );
                            let _=done.send(PublishCompletion{rate_limit}).await;
                            let _=publication.response.send(result);
                            inflight.store(false,Ordering::SeqCst);
                        });
                        continue;
                    }
                    if let Some(id)=next_subscription {
                        let filters=desired.get(&id).map(|entry|entry.filters.clone()).unwrap_or_default();
                        match handle.subscribe(id.clone(),filters).await {
                            Ok(())=>{
                                if let Some(entry)=desired.get_mut(&id) {
                                    entry.needs_send=false;
                                    entry.retry_at=None;
                                }
                                wire_open.insert(id);
                            }
                            Err(_)=>break,
                        }
                    }
                }
            }
        }
    }

    publications.reject_all("supervisor stopped before publication admission");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn desired_selection_is_priority_then_stable_id() {
        let now = Instant::now();
        let mut desired = BTreeMap::new();
        remember_subscription(
            &mut desired,
            "z-background".into(),
            vec![json!({})],
            SubscriptionPriority::Background,
        );
        remember_subscription(
            &mut desired,
            "baseline".into(),
            vec![json!({})],
            SubscriptionPriority::Baseline,
        );
        remember_subscription(
            &mut desired,
            "foreground".into(),
            vec![json!({})],
            SubscriptionPriority::Foreground,
        );
        assert_eq!(
            next_subscription(&desired, now).as_deref(),
            Some("foreground")
        );
        desired.get_mut("foreground").unwrap().needs_send = false;
        assert_eq!(
            next_subscription(&desired, now).as_deref(),
            Some("baseline")
        );
    }

    #[test]
    fn same_id_replacement_keeps_only_latest_filters() {
        let mut desired = BTreeMap::new();
        remember_subscription(
            &mut desired,
            "same".into(),
            vec![json!({"kinds":[9]})],
            SubscriptionPriority::Background,
        );
        remember_subscription(
            &mut desired,
            "same".into(),
            vec![json!({"kinds":[20_002]})],
            SubscriptionPriority::Foreground,
        );
        assert_eq!(desired.len(), 1);
        assert_eq!(desired["same"].filters[0]["kinds"], json!([20_002]));
        assert_eq!(desired["same"].priority, SubscriptionPriority::Foreground);
    }

    #[test]
    fn expired_unsent_publication_is_definitively_local() {
        let (response, mut result) = oneshot::channel();
        let event = nostr::EventBuilder::text_note("generated")
            .sign_with_keys(&nostr::Keys::generate())
            .unwrap();
        let now = Instant::now();
        let mut queues = PublicationQueues::default();
        queues.push(
            PublishPriority::Interactive,
            PendingPublication {
                event,
                queued_at: now - admission::PUBLICATION_QUEUE_TIMEOUT,
                response,
            },
        );
        queues.reject_expired(now);
        assert!(matches!(
            result.try_recv(),
            Ok(Err(ref error)) if admission::is_local_admission_error(error)
        ));
        assert_eq!(queues.len(), 0);
    }

    #[test]
    fn lane_priority_keeps_selected_and_baseline_ahead_of_maintenance() {
        assert_eq!(
            admission_lane(false, true, Some(SubscriptionPriority::Foreground), true,),
            AdmissionLane::InteractivePublication
        );
        assert_eq!(
            admission_lane(false, false, Some(SubscriptionPriority::Foreground), true,),
            AdmissionLane::Subscription
        );
        assert_eq!(
            admission_lane(false, false, Some(SubscriptionPriority::Baseline), true,),
            AdmissionLane::Subscription
        );
        assert_eq!(
            admission_lane(false, false, Some(SubscriptionPriority::Background), true,),
            AdmissionLane::DeferredPublication
        );
    }

    #[test]
    fn publication_queues_prefer_interactive_then_recovery() {
        fn item() -> PendingPublication {
            let (response, _) = oneshot::channel();
            PendingPublication {
                event: nostr::EventBuilder::text_note("generated")
                    .sign_with_keys(&nostr::Keys::generate())
                    .unwrap(),
                queued_at: Instant::now(),
                response,
            }
        }
        let mut queues = PublicationQueues::default();
        queues.push(PublishPriority::Maintenance, item());
        queues.push(PublishPriority::Recovery, item());
        queues.push(PublishPriority::Interactive, item());
        assert_eq!(queues.pop().unwrap().event.content, "generated");
        assert_eq!(queues.interactive.len(), 0);
        assert_eq!(queues.recovery.len(), 1);
        assert_eq!(queues.maintenance.len(), 1);
    }
}
