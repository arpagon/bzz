//! Executes broker actions on the relay as the agent. Each hosted agent has
//! its own signer, its own NIP-42-authenticated connection and its own NIP-98
//! HTTP client, so agent traffic never shares the human's admission budget;
//! a per-agent write budget keeps it slower still. Writes require the
//! relay-signed `bot` role in the target channel; the relay stays the final
//! authority. Every agent event carries the owner's NIP-OA `auth` tag.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use buzz_sdk::broker::{
    ActionArgs, ActionOutcome, BrokerError, BrokerErrorCode, BrokerMessage, EventPublished,
    MessagePage, PubkeyHex, StorageAddress,
};
use nostr::{Event, EventBuilder, EventId, Tag};
use serde_json::json;
use uuid::Uuid;

use crate::{
    auth::signer::SignerHandle,
    broker::host::{ExecError, Executor, HostedAgent},
    protocol::http::HttpClient,
    realtime::supervisor::SupervisorHandle,
    store::writer::StoreHandle,
};

/// Writes per agent: a burst of 5, refilled one per two seconds.
const WRITE_BURST: f64 = 5.0;
const WRITE_REFILL_PER_SECOND: f64 = 0.5;
const ACK_TIMEOUT: Duration = Duration::from_secs(15);
/// Kind 30174 NIP-AE agent memory records.
const MEMORY_KIND: u32 = 30_174;

pub struct AgentRuntime {
    pub signer: SignerHandle,
    pub http: HttpClient,
    pub supervisor: SupervisorHandle,
    pub owner: nostr::PublicKey,
    pub auth_tag: Tag,
}

struct Bucket {
    tokens: f64,
    at: Instant,
}

pub struct RelayExecutor {
    community: Uuid,
    store: StoreHandle,
    agents: HashMap<Uuid, AgentRuntime>,
    buckets: Mutex<HashMap<Uuid, Bucket>>,
}

/// Parses the stored compact NIP-OA tag into an event tag.
pub fn auth_tag(json: &str) -> crate::error::Result<Tag> {
    let parts = serde_json::from_str::<Vec<String>>(json).map_err(|_| {
        crate::error::Error::IdentityCorrupt("owner authorization is malformed".into())
    })?;
    Tag::parse(parts).map_err(|error| crate::error::Error::IdentityCorrupt(error.to_string()))
}

fn failed(message: impl Into<String>) -> ExecError {
    ExecError::action_failed(message)
}

fn sdk(error: buzz_sdk::SdkError) -> ExecError {
    ExecError::Failed(BrokerError::invalid_request(error.to_string()))
}

impl RelayExecutor {
    pub fn new(community: Uuid, store: StoreHandle, agents: HashMap<Uuid, AgentRuntime>) -> Self {
        Self {
            community,
            store,
            agents,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn runtime(&self, agent: &HostedAgent) -> Result<&AgentRuntime, ExecError> {
        self.agents
            .get(&agent.id)
            .ok_or_else(|| ExecError::unauthorized("this agent is not unlocked on this host"))
    }

    fn take_write(&self, agent: Uuid) -> Result<(), ExecError> {
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let bucket = buckets.entry(agent).or_insert(Bucket {
            tokens: WRITE_BURST,
            at: now,
        });
        let elapsed = now.duration_since(bucket.at).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * WRITE_REFILL_PER_SECOND).min(WRITE_BURST);
        bucket.at = now;
        if bucket.tokens < 1.0 {
            return Err(failed("rate limited by the broker; retry later"));
        }
        bucket.tokens -= 1.0;
        Ok(())
    }

    async fn require_bot(&self, agent: &HostedAgent, channel: Uuid) -> Result<(), ExecError> {
        let community = self.community;
        let pubkey = agent.pubkey.clone();
        let is_bot = self
            .store
            .call(move |store| store.member_role(community, channel, &pubkey))
            .await
            .map_err(|_| ExecError::Failed(BrokerError::new(BrokerErrorCode::Internal, "store")))?
            .is_some_and(|role| role == "bot");
        if is_bot {
            Ok(())
        } else {
            Err(ExecError::unauthorized(
                "the agent has no bot role in this channel",
            ))
        }
    }

    async fn event(runtime: &AgentRuntime, id: &str) -> Result<Event, ExecError> {
        runtime
            .http
            .query_raw(&[json!({"ids": [id], "limit": 1})])
            .await
            .map_err(|_| failed("the target event could not be read"))?
            .into_iter()
            .find(|event| event.id.to_hex() == id && crate::protocol::events::verify(event).is_ok())
            .ok_or_else(|| failed("the target event is not readable by this agent"))
    }

    async fn publish(
        &self,
        agent: &HostedAgent,
        runtime: &AgentRuntime,
        builder: EventBuilder,
    ) -> Result<EventPublished, ExecError> {
        self.take_write(agent.id)?;
        let event = runtime
            .signer
            .sign(builder.tag(runtime.auth_tag.clone()))
            .await
            .map_err(|_| {
                ExecError::Failed(BrokerError::new(
                    BrokerErrorCode::Internal,
                    "signing failed",
                ))
            })?;
        let published = EventPublished {
            event_id: event.id.to_hex(),
            kind: u32::from(event.kind.as_u16()),
            created_at: event.created_at.as_secs(),
        };
        match tokio::time::timeout(ACK_TIMEOUT, runtime.supervisor.publish(event)).await {
            Ok(Ok(ack)) if ack.accepted => Ok(published),
            Ok(Ok(ack)) => Err(failed(format!(
                "the relay rejected the event: {}",
                crate::protocol::artifact::display(&ack.message, 160)
            ))),
            // Sent or maybe sent, with no verdict: the action may have run.
            _ => Err(ExecError::Indeterminate(BrokerError::new(
                BrokerErrorCode::OutcomeUnknown,
                "no relay acknowledgement",
            ))),
        }
    }

    async fn read(
        runtime: &AgentRuntime,
        agent: &HostedAgent,
        args: &buzz_sdk::broker::ChannelReadArgs,
    ) -> Result<MessagePage, ExecError> {
        let limit = args.limit.unwrap_or(100).clamp(1, 500) as usize;
        let cursor = args
            .cursor
            .as_deref()
            .map(|cursor| {
                let (at, id) = cursor.split_once(':').ok_or(())?;
                Ok::<_, ()>((at.parse::<u64>().map_err(|_| ())?, id.to_owned()))
            })
            .transpose()
            .map_err(|()| ExecError::Failed(BrokerError::invalid_request("unknown cursor")))?;
        let mut filter = json!({"kinds": [9], "#h": [args.channel_id], "limit": limit + 50});
        if let Some((at, _)) = &cursor {
            filter["until"] = json!(at);
        }
        if let Some(root) = &args.root_event_id {
            filter["#e"] = json!([root]);
        }
        if args.mentions_only {
            filter["#p"] = json!([agent.pubkey]);
        }
        let mut events = runtime
            .http
            .query_raw(&[filter])
            .await
            .map_err(|_| failed("the channel could not be read by this agent"))?
            .into_iter()
            .filter(|event| crate::protocol::events::verify(event).is_ok())
            .filter(|event| {
                cursor.as_ref().is_none_or(|(at, id)| {
                    (event.created_at.as_secs(), event.id.to_hex()) < (*at, id.clone())
                })
            })
            .collect::<Vec<_>>();
        // Newest first, ties by id; the cursor is the last row's position.
        events.sort_by(|left, right| {
            (right.created_at, right.id.to_hex()).cmp(&(left.created_at, left.id.to_hex()))
        });
        events.dedup_by(|left, right| left.id == right.id);
        let more = events.len() > limit;
        events.truncate(limit);
        let next_cursor = more
            .then(|| events.last())
            .flatten()
            .map(|event| format!("{}:{}", event.created_at.as_secs(), event.id.to_hex()));
        Ok(MessagePage {
            messages: events.into_iter().map(BrokerMessage).collect(),
            next_cursor,
        })
    }
}

fn channel(value: &str) -> Result<Uuid, ExecError> {
    Uuid::parse_str(value).map_err(|_| ExecError::Failed(BrokerError::invalid_request("channelId")))
}

fn event_id(value: &str) -> Result<EventId, ExecError> {
    EventId::from_hex(value)
        .map_err(|_| ExecError::Failed(BrokerError::invalid_request("event id")))
}

fn in_channel(event: &Event, channel: Uuid) -> bool {
    crate::protocol::events::first_tag(event, "h").as_deref() == Some(channel.to_string().as_str())
}

fn mentions(values: &[PubkeyHex]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.as_str().to_owned())
        .collect()
}

#[async_trait]
impl Executor for RelayExecutor {
    async fn execute(
        &self,
        agent: &HostedAgent,
        args: &ActionArgs,
    ) -> Result<ActionOutcome, ExecError> {
        let runtime = self.runtime(agent)?;
        match args {
            ActionArgs::ChannelRead(args) => Ok(ActionOutcome::ChannelRead(
                Self::read(runtime, agent, args).await?,
            )),
            ActionArgs::MessagePost(args) => {
                let channel = channel(&args.channel_id)?;
                self.require_bot(agent, channel).await?;
                let mentions = mentions(&args.mentions);
                let mentions = mentions.iter().map(String::as_str).collect::<Vec<_>>();
                let builder = buzz_sdk::build_message(
                    channel,
                    &args.content,
                    None,
                    &mentions,
                    false,
                    &[],
                    &[],
                )
                .map_err(sdk)?;
                Ok(ActionOutcome::MessagePost(
                    self.publish(agent, runtime, builder).await?,
                ))
            }
            ActionArgs::MessageReply(args) => {
                let channel = channel(&args.channel_id)?;
                self.require_bot(agent, channel).await?;
                let parent = Self::event(runtime, &args.reply_to_event_id).await?;
                if !in_channel(&parent, channel) {
                    return Err(failed("the replied-to event is not in this channel"));
                }
                let root = crate::protocol::events::thread_coordinates(&parent)
                    .0
                    .unwrap_or_else(|| parent.id.to_hex());
                let thread = buzz_sdk::ThreadRef {
                    root_event_id: event_id(&root)?,
                    parent_event_id: parent.id,
                };
                let mentions = mentions(&args.mentions);
                let mentions = mentions.iter().map(String::as_str).collect::<Vec<_>>();
                let builder = buzz_sdk::build_message(
                    channel,
                    &args.content,
                    Some(&thread),
                    &mentions,
                    false,
                    &[],
                    &[],
                )
                .map_err(sdk)?;
                Ok(ActionOutcome::MessageReply(
                    self.publish(agent, runtime, builder).await?,
                ))
            }
            ActionArgs::ReactionAdd(args) => {
                let channel = channel(&args.channel_id)?;
                self.require_bot(agent, channel).await?;
                let target = Self::event(runtime, &args.target_event_id).await?;
                if !in_channel(&target, channel) {
                    return Err(failed("the target event is not in this channel"));
                }
                let builder = buzz_sdk::build_reaction(target.id, &args.reaction).map_err(sdk)?;
                Ok(ActionOutcome::ReactionAdd(
                    self.publish(agent, runtime, builder).await?,
                ))
            }
            ActionArgs::ProfileSet(args) => {
                // Merge onto the current profile: omitted fields stay.
                let current = runtime
                    .http
                    .query_raw(&[json!({"kinds": [0], "authors": [agent.pubkey], "limit": 1})])
                    .await
                    .ok()
                    .and_then(|events| events.into_iter().max_by_key(|event| event.created_at))
                    .and_then(|event| {
                        serde_json::from_str::<serde_json::Value>(&event.content).ok()
                    })
                    .unwrap_or_else(|| json!({}));
                let field = |name: &str| current[name].as_str().map(str::to_owned);
                let display = args.display_name.clone().or_else(|| field("display_name"));
                let about = args.about.clone().or_else(|| field("about"));
                let picture = args.picture.clone().or_else(|| field("picture"));
                let builder = buzz_sdk::build_profile(
                    display.as_deref(),
                    field("name").as_deref(),
                    picture.as_deref(),
                    about.as_deref(),
                    field("nip05").as_deref(),
                )
                .map_err(sdk)?;
                Ok(ActionOutcome::ProfileSet(
                    self.publish(agent, runtime, builder).await?,
                ))
            }
            ActionArgs::StorageAddress(args) => {
                let d_tag = runtime
                    .signer
                    .memory_d_tag(runtime.owner, &args.slug)
                    .await
                    .map_err(|_| {
                        ExecError::Failed(BrokerError::new(
                            BrokerErrorCode::Internal,
                            "derivation failed",
                        ))
                    })?;
                Ok(ActionOutcome::StorageAddress(StorageAddress {
                    author_pubkey: PubkeyHex::parse(&agent.pubkey).map_err(sdk)?,
                    kind: MEMORY_KIND,
                    d_tag,
                }))
            }
            ActionArgs::AgentsCreate(_)
            | ActionArgs::AgentsUpdate(_)
            | ActionArgs::AgentsDelete(_) => Err(ExecError::Failed(BrokerError::unsupported(
                "this host does not run managed agents",
            ))),
        }
    }

    fn target_label(&self, args: &ActionArgs) -> Option<String> {
        let channel = match args {
            ActionArgs::ChannelRead(args) => &args.channel_id,
            ActionArgs::MessagePost(args) => &args.channel_id,
            ActionArgs::MessageReply(args) => &args.channel_id,
            ActionArgs::ReactionAdd(args) => &args.channel_id,
            _ => return None,
        };
        Some(format!(
            "channel {}",
            channel.chars().take(8).collect::<String>()
        ))
    }
}
