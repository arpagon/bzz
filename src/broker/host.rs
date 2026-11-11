//! `buzz-sdk::broker` host: authenticate → authorize → validate → execute.
//!
//! The request body never names its subject; the bearer credential alone
//! resolves the agent. Credentials are stored only as SHA-256 hashes and are
//! compared in constant time. No secret key, credential or content enters a
//! response message or the activity log.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use buzz_sdk::broker::{
    Action, ActionArgs, ActionOutcome, BROKER_PROTOCOL_VERSION, BROKER_REQUEST_TYPE, BrokerError,
    BrokerErrorCode, BrokerRequest, BrokerResponse, BrokerResult, validate_request_id,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Requests remembered per agent for idempotent replay.
const IDEMPOTENCY_ENTRIES: usize = 1024;
/// Activity log entries kept (content-free).
pub const ACTIVITY_ENTRIES: usize = 500;
/// The largest request body the host reads.
pub const MAX_BODY_BYTES: usize = 256 * 1024;

/// One hosted agent as the host sees it: never its secret key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostedAgent {
    pub id: Uuid,
    pub label: String,
    pub pubkey: String,
    /// Lowercase hex SHA-256 of the bearer credential.
    pub credential_hash: String,
    /// Allowed action names (`channel.read`, …).
    pub allowed: BTreeSet<String>,
    pub paused: bool,
}

pub fn credential_hash(credential: &str) -> String {
    hex::encode(Sha256::digest(credential.as_bytes()))
}

/// A fresh 256-bit credential, shown once to the owner.
pub fn new_credential() -> String {
    format!("bzzb_{}", hex::encode(rand::random::<[u8; 32]>()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// Whether the failure left side effects possible.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecError {
    /// Nothing happened (relay rejection, denial, invalid target).
    Failed(BrokerError),
    /// The action may have happened (no acknowledgement).
    Indeterminate(BrokerError),
}

impl ExecError {
    pub fn action_failed(message: impl Into<String>) -> Self {
        Self::Failed(BrokerError::new(BrokerErrorCode::ActionFailed, message))
    }
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Failed(BrokerError::unauthorized(message))
    }
}

/// Performs validated actions for an authenticated, authorized agent.
#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute(
        &self,
        agent: &HostedAgent,
        args: &ActionArgs,
    ) -> std::result::Result<ActionOutcome, ExecError>;
    /// A short, content-free label of the action's target (channel name).
    fn target_label(&self, args: &ActionArgs) -> Option<String>;
}

/// One content-free activity row: what an agent did, never what it said.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Activity {
    pub at: u64,
    pub agent: String,
    pub action: String,
    pub outcome: String,
    pub target: Option<String>,
}

#[derive(Default)]
struct State {
    agents: Vec<HostedAgent>,
    /// `(agent id, request id)` -> (body digest, recorded result).
    replay: HashMap<(Uuid, String), (String, BrokerResult)>,
    replay_order: HashMap<Uuid, VecDeque<String>>,
    activity: VecDeque<Activity>,
    paused_all: bool,
}

#[derive(Clone)]
pub struct Host {
    state: Arc<Mutex<State>>,
    executor: Arc<dyn Executor>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The broker's own action names. `agents.*` create managed agents that some
/// runtime must run; bzz runs none (ADR v0.13 agent broker), so those three
/// are answered `unsupported`, which the contract allows for any action.
pub fn hosted_actions() -> BTreeSet<String> {
    [
        Action::ChannelRead,
        Action::MessagePost,
        Action::MessageReply,
        Action::ReactionAdd,
        Action::ProfileSet,
        Action::StorageAddress,
    ]
    .into_iter()
    .map(|action| action.as_str().to_owned())
    .collect()
}

impl Host {
    pub fn new(executor: Arc<dyn Executor>, agents: Vec<HostedAgent>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                agents,
                ..State::default()
            })),
            executor,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_agents(&self, agents: Vec<HostedAgent>) {
        self.lock().agents = agents;
    }

    /// The owner changed one served agent's policy; others are untouched and
    /// an agent this host does not serve is not added.
    pub fn update_policy(&self, id: Uuid, allowed: BTreeSet<String>, paused: bool) {
        if let Some(agent) = self.lock().agents.iter_mut().find(|agent| agent.id == id) {
            agent.allowed = allowed;
            agent.paused = paused;
        }
    }

    /// The owner's one-key pause for every hosted agent.
    pub fn set_paused_all(&self, paused: bool) {
        self.lock().paused_all = paused;
    }

    pub fn paused_all(&self) -> bool {
        self.lock().paused_all
    }

    pub fn activity(&self) -> Vec<Activity> {
        self.lock().activity.iter().cloned().collect()
    }

    fn record(
        &self,
        agent: &HostedAgent,
        action: &str,
        result: &BrokerResult,
        target: Option<String>,
    ) {
        let outcome = match result {
            BrokerResult::Succeeded { .. } => "succeeded".to_owned(),
            BrokerResult::Failed { error } => format!("failed:{}", error.code.as_str()),
            BrokerResult::Indeterminate { error } => {
                format!("indeterminate:{}", error.code.as_str())
            }
        };
        let mut state = self.lock();
        state.activity.push_back(Activity {
            at: now(),
            agent: agent.label.clone(),
            action: action.to_owned(),
            outcome,
            target,
        });
        while state.activity.len() > ACTIVITY_ENTRIES {
            state.activity.pop_front();
        }
    }

    /// Handles one `POST /v1/action`. Always returns an envelope (HTTP 200).
    pub async fn handle(&self, authorization: Option<&str>, body: &[u8]) -> BrokerResponse {
        // Only the request id is read before authentication: the response
        // must correlate, and nothing else of an unauthenticated body is used.
        let value = serde_json::from_slice::<serde_json::Value>(body).ok();
        let request_id = value
            .as_ref()
            .and_then(|value| value.get("requestId"))
            .and_then(serde_json::Value::as_str)
            .filter(|id| validate_request_id(id).is_ok())
            .unwrap_or("-")
            .to_owned();
        let fail = |code, message: &str| {
            BrokerResponse::new(
                request_id.clone(),
                BrokerResult::failed(BrokerError::new(code, message)),
            )
        };
        let Some(agent) = self.authenticate(authorization) else {
            return fail(
                BrokerErrorCode::Unauthenticated,
                "unknown or revoked credential",
            );
        };
        let Some(value) = value else {
            return fail(BrokerErrorCode::InvalidRequest, "body is not JSON");
        };
        if request_id == "-" {
            return fail(
                BrokerErrorCode::InvalidRequest,
                "missing or invalid requestId",
            );
        }
        let digest = hex::encode(Sha256::digest(body));
        if let Some((recorded, result)) = self.lock().replay.get(&(agent.id, request_id.clone())) {
            return if *recorded == digest {
                BrokerResponse::new(request_id, result.clone()).replayed()
            } else {
                fail(
                    BrokerErrorCode::RequestIdConflict,
                    "requestId was used with different bytes",
                )
            };
        }
        let request = match serde_json::from_value::<BrokerRequest>(value.clone()) {
            Ok(request) => request,
            Err(_) => {
                return fail(
                    classify(&value),
                    "request does not match the broker contract",
                );
            }
        };
        if request.r#type != BROKER_REQUEST_TYPE {
            return fail(BrokerErrorCode::InvalidRequest, "wrong request type");
        }
        if request.protocol_version != BROKER_PROTOCOL_VERSION {
            return fail(
                BrokerErrorCode::UnsupportedProtocolVersion,
                "protocolVersion must be 1",
            );
        }
        let action = request.action();
        if request.action_version != action.current_version() {
            return fail(
                BrokerErrorCode::UnsupportedActionVersion,
                "actionVersion is not supported",
            );
        }
        let name = action.as_str();
        // Authorize before validating content.
        let result = if self.paused_all() || agent.paused {
            BrokerResult::failed(BrokerError::unauthorized("the owner paused this agent"))
        } else if !hosted_actions().contains(name) {
            BrokerResult::failed(BrokerError::unsupported(
                "this host does not run managed agents",
            ))
        } else if !agent.allowed.contains(name) {
            BrokerResult::failed(BrokerError::unauthorized(
                "the owner has not allowed this action for this agent",
            ))
        } else {
            match request.validated() {
                Err(error) => BrokerResult::failed(BrokerError::invalid_request(error.to_string())),
                Ok(validated) => match self.executor.execute(&agent, validated.args()).await {
                    Ok(outcome) => BrokerResult::succeeded(outcome),
                    Err(ExecError::Failed(error)) => BrokerResult::failed(error),
                    Err(ExecError::Indeterminate(error)) => BrokerResult::indeterminate(error),
                },
            }
        };
        let target = request_target(&self.executor, &value);
        self.record(&agent, name, &result, target);
        self.remember(agent.id, request_id.clone(), digest, result.clone());
        BrokerResponse::new(request_id, result)
    }

    fn authenticate(&self, authorization: Option<&str>) -> Option<HostedAgent> {
        let token = authorization?.strip_prefix("Bearer ")?.trim();
        if token.is_empty() {
            return None;
        }
        let hash = credential_hash(token);
        let state = self.lock();
        let mut found = None;
        // Compare against every agent so timing does not depend on position.
        for agent in &state.agents {
            if constant_time_eq(agent.credential_hash.as_bytes(), hash.as_bytes()) {
                found = Some(agent.clone());
            }
        }
        found
    }

    fn remember(&self, agent: Uuid, request_id: String, digest: String, result: BrokerResult) {
        let mut state = self.lock();
        let order = state.replay_order.entry(agent).or_default();
        order.push_back(request_id.clone());
        let evicted = (order.len() > IDEMPOTENCY_ENTRIES)
            .then(|| order.pop_front())
            .flatten();
        if let Some(evicted) = evicted {
            state.replay.remove(&(agent, evicted));
        }
        state.replay.insert((agent, request_id), (digest, result));
    }
}

fn request_target(executor: &Arc<dyn Executor>, value: &serde_json::Value) -> Option<String> {
    let request = serde_json::from_value::<BrokerRequest>(value.clone()).ok()?;
    executor.target_label(&request.action)
}

/// Maps a body that failed the strict parse to the most specific code.
fn classify(value: &serde_json::Value) -> BrokerErrorCode {
    if value
        .get("protocolVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(u64::from(BROKER_PROTOCOL_VERSION))
    {
        return BrokerErrorCode::UnsupportedProtocolVersion;
    }
    let known = [
        Action::ChannelRead,
        Action::MessagePost,
        Action::MessageReply,
        Action::ReactionAdd,
        Action::ProfileSet,
        Action::StorageAddress,
        Action::AgentsCreate,
        Action::AgentsUpdate,
        Action::AgentsDelete,
    ];
    let action = value.get("action").and_then(serde_json::Value::as_str);
    if action.is_none_or(|name| !known.iter().any(|known| known.as_str() == name)) {
        return BrokerErrorCode::UnknownAction;
    }
    if value
        .get("actionVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return BrokerErrorCode::UnsupportedActionVersion;
    }
    BrokerErrorCode::InvalidRequest
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_sdk::broker::{
        BrokerClient, BrokerClientExt, BrokerFuture, ChannelReadArgs, Dispatch, EventPublished,
        MessagePage, MessagePostArgs, PreparedRequest, StorageAddressArgs,
    };

    struct Fake;

    #[async_trait]
    impl Executor for Fake {
        async fn execute(
            &self,
            _agent: &HostedAgent,
            args: &ActionArgs,
        ) -> std::result::Result<ActionOutcome, ExecError> {
            match args {
                ActionArgs::ChannelRead(_) => Ok(ActionOutcome::ChannelRead(MessagePage {
                    messages: Vec::new(),
                    next_cursor: None,
                })),
                ActionArgs::MessagePost(args) if args.content == "indeterminate" => {
                    Err(ExecError::Indeterminate(BrokerError::new(
                        BrokerErrorCode::OutcomeUnknown,
                        "no acknowledgement",
                    )))
                }
                ActionArgs::MessagePost(args) if args.content == "not a bot" => Err(
                    ExecError::unauthorized("the agent has no bot role in this channel"),
                ),
                ActionArgs::MessagePost(_) => Ok(ActionOutcome::MessagePost(EventPublished {
                    event_id: "a".repeat(64),
                    kind: 9,
                    created_at: 1,
                })),
                _ => Err(ExecError::action_failed("not faked")),
            }
        }
        fn target_label(&self, _args: &ActionArgs) -> Option<String> {
            Some("#general".into())
        }
    }

    /// The SDK's own client and response validation drive the host.
    struct InProcess(Host, String);

    impl BrokerClient for InProcess {
        fn send<'a>(
            &'a self,
            request: &'a PreparedRequest,
            _dispatch: Dispatch,
        ) -> BrokerFuture<'a> {
            Box::pin(async move {
                let response = self
                    .0
                    .handle(Some(&format!("Bearer {}", self.1)), request.body())
                    .await;
                // Through the wire, exactly as the HTTP transport would.
                let bytes = serde_json::to_vec(&response).unwrap();
                Ok(serde_json::from_slice(&bytes).unwrap())
            })
        }
    }

    fn agent(credential: &str) -> HostedAgent {
        HostedAgent {
            id: Uuid::new_v4(),
            label: "helper".into(),
            pubkey: "b".repeat(64),
            credential_hash: credential_hash(credential),
            allowed: ["channel.read", "message.post", "storage.address"]
                .map(str::to_owned)
                .into(),
            paused: false,
        }
    }

    fn post(id: &str, content: &str) -> PreparedRequest {
        BrokerRequest::new(
            id,
            ActionArgs::MessagePost(MessagePostArgs {
                channel_id: Uuid::new_v4().to_string(),
                content: content.into(),
                mentions: Vec::new(),
            }),
        )
        .unwrap()
        .prepare()
        .unwrap()
    }

    fn code(response: &buzz_sdk::broker::ValidatedResponse) -> Option<BrokerErrorCode> {
        response.result().error().map(|error| error.code)
    }

    #[tokio::test]
    async fn sdk_client_validates_every_host_answer() {
        let credential = new_credential();
        let host = Host::new(Arc::new(Fake), vec![agent(&credential)]);
        let client = InProcess(host.clone(), credential.clone());

        let read = BrokerRequest::new(
            "r1",
            ActionArgs::ChannelRead(ChannelReadArgs {
                channel_id: Uuid::new_v4().to_string(),
                root_event_id: None,
                mentions_only: false,
                cursor: None,
                limit: Some(10),
            }),
        )
        .unwrap()
        .prepare()
        .unwrap();
        let response = client.execute(&read).await.unwrap();
        assert!(response.result().outcome().is_some());

        // Idempotent replay of identical bytes; conflict on different bytes.
        let first = post("p1", "hello");
        assert!(
            client
                .execute(&first)
                .await
                .unwrap()
                .result()
                .outcome()
                .is_some()
        );
        let again = client.execute(&first).await.unwrap();
        assert!(again.replayed());
        let conflict = client.execute(&post("p1", "other")).await.unwrap();
        assert_eq!(code(&conflict), Some(BrokerErrorCode::RequestIdConflict));

        // Typed denials and indeterminate outcomes.
        let denied = client.execute(&post("p2", "not a bot")).await.unwrap();
        assert_eq!(code(&denied), Some(BrokerErrorCode::Unauthorized));
        let unknown = client.execute(&post("p3", "indeterminate")).await.unwrap();
        assert!(matches!(
            unknown.result(),
            BrokerResult::Indeterminate { .. }
        ));
        let not_allowed = BrokerRequest::new(
            "s1",
            ActionArgs::StorageAddress(StorageAddressArgs {
                slug: "core".into(),
            }),
        )
        .unwrap()
        .prepare()
        .unwrap();
        assert!(
            client.execute(&not_allowed).await.is_ok(),
            "allowed, fake fails"
        );
        let managed = BrokerRequest::new(
            "a1",
            ActionArgs::AgentsDelete(buzz_sdk::broker::AgentsDeleteArgs {
                target: buzz_sdk::broker::AgentTarget::Name("x".into()),
            }),
        )
        .unwrap()
        .prepare()
        .unwrap();
        assert_eq!(
            code(&client.execute(&managed).await.unwrap()),
            Some(BrokerErrorCode::Unsupported)
        );

        // A panel policy edit applies to the served agent at once: denying
        // message.post refuses the next post; pausing one agent refuses all.
        let served = host.lock().agents[0].id;
        host.update_policy(served, ["channel.read".to_owned()].into(), false);
        let revoked = client.execute(&post("p6", "hello")).await.unwrap();
        assert_eq!(code(&revoked), Some(BrokerErrorCode::Unauthorized));
        host.update_policy(Uuid::new_v4(), BTreeSet::new(), true);
        assert_eq!(
            host.lock().agents.len(),
            1,
            "an unserved agent is not added"
        );
        host.update_policy(
            served,
            ["channel.read", "message.post", "storage.address"]
                .map(str::to_owned)
                .into(),
            false,
        );

        // Pause: every action is refused with a typed denial.
        host.set_paused_all(true);
        let paused = client.execute(&post("p4", "hello")).await.unwrap();
        assert_eq!(code(&paused), Some(BrokerErrorCode::Unauthorized));
        host.set_paused_all(false);

        // Unknown credential: unauthenticated, nothing ran or was logged.
        let before = host.activity().len();
        let stranger = InProcess(host.clone(), new_credential());
        let rejected = stranger.execute(&post("p5", "hello")).await.unwrap();
        assert_eq!(code(&rejected), Some(BrokerErrorCode::Unauthenticated));
        assert_eq!(host.activity().len(), before);

        // The activity log is content-free.
        let activity = host.activity();
        assert!(
            activity
                .iter()
                .all(|row| row.target.as_deref() == Some("#general"))
        );
        let rendered = format!("{activity:?}");
        assert!(!rendered.contains("hello") && !rendered.contains(&credential));
    }

    #[tokio::test]
    async fn malformed_bodies_get_the_specific_code() {
        let credential = new_credential();
        let host = Host::new(Arc::new(Fake), vec![agent(&credential)]);
        let bearer = format!("Bearer {credential}");
        let call = |body: serde_json::Value| {
            let host = host.clone();
            let bearer = bearer.clone();
            async move {
                let response = host
                    .handle(Some(&bearer), &serde_json::to_vec(&body).unwrap())
                    .await;
                response.result.error().map(|error| error.code)
            }
        };
        let base = serde_json::json!({"type":"broker_request","protocolVersion":1,"requestId":"x","actionVersion":1,"action":"channel.read","args":{"channelId":Uuid::new_v4().to_string()}});
        let mut body = base.clone();
        body["protocolVersion"] = 2.into();
        assert_eq!(
            call(body).await,
            Some(BrokerErrorCode::UnsupportedProtocolVersion)
        );
        let mut body = base.clone();
        body["action"] = "sign.bytes".into();
        assert_eq!(call(body).await, Some(BrokerErrorCode::UnknownAction));
        let mut body = base.clone();
        body["actionVersion"] = 9.into();
        assert_eq!(
            call(body).await,
            Some(BrokerErrorCode::UnsupportedActionVersion)
        );
        let mut body = base.clone();
        body["args"]["extra"] = 1.into();
        assert_eq!(call(body).await, Some(BrokerErrorCode::InvalidRequest));
        let mut body = base;
        body["args"]["cursor"] = serde_json::Value::Null;
        assert_eq!(
            call(body).await,
            Some(BrokerErrorCode::InvalidRequest),
            "explicit null"
        );
    }
}
