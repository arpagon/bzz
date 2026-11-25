//! The generated world of `bzz demo`: the people of bzz's films (Ada, Lin,
//! Sam, the agent Scout, and You) in a small team that ships Harbor 2.4, a
//! made-up status-page tool, on Thursday.
//!
//! Every key but the viewer's comes from a fixed seed, so each run shows the
//! same team. The viewer's key is new on every run and never leaves memory:
//! the demo relay serves only the process that holds it. Nothing here comes
//! from this machine or its owner, and the only address in the world is the
//! demo relay's own loopback origin.

use std::{collections::HashMap, time::Duration};

use nostr::{Event, EventBuilder, EventId, Keys, Kind, Metadata, PublicKey, Tag, Timestamp};
use serde_json::json;
use sha2::Digest as _;
use uuid::Uuid;

/// The demo's people, in the order the films name them.
pub struct Cast {
    pub ada: Keys,
    pub lin: Keys,
    pub sam: Keys,
    /// The agent, owned by Ada.
    pub scout: Keys,
}

/// The same four keys on every run.
impl Default for Cast {
    fn default() -> Self {
        Self::new()
    }
}

impl Cast {
    fn new() -> Self {
        Self {
            ada: seeded(0x0a),
            lin: seeded(0x0b),
            sam: seeded(0x0c),
            scout: seeded(0x0d),
        }
    }
}

/// Everything the relay serves and the store is seeded with.
pub struct World {
    /// Signed events, oldest first.
    pub events: Vec<Event>,
    /// `sha256.jpg` → bytes, served at `/media/<name>`.
    pub media: HashMap<String, Vec<u8>>,
    pub relay_keys: Keys,
    pub viewer: Keys,
    pub team: Uuid,
    pub script: Script,
}

/// A fixed key per person: the world is the same on every run.
fn seeded(seed: u8) -> Keys {
    Keys::parse(&format!("{seed:02x}").repeat(32)).expect("a fixed demo key is valid")
}

/// A fixed channel id, derived from the channel's name.
fn channel_id(name: &str) -> Uuid {
    let digest = sha2::Sha256::digest(format!("bzz-demo/{name}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

const AVATARS: [(&str, &[u8]); 5] = [
    ("ada", include_bytes!("assets/ada.jpg")),
    ("lin", include_bytes!("assets/lin.jpg")),
    ("sam", include_bytes!("assets/sam.jpg")),
    ("scout", include_bytes!("assets/scout.jpg")),
    ("you", include_bytes!("assets/you.jpg")),
];

fn tag(parts: &[&str]) -> Tag {
    Tag::parse(parts.iter().copied()).expect("a demo tag is well formed")
}

/// Signs the world's events at fixed distances from now.
struct Pen {
    now: u64,
    events: Vec<Event>,
}

impl Pen {
    fn sign(&mut self, keys: &Keys, builder: EventBuilder, minutes_ago: u64) -> Event {
        let event = builder
            .custom_created_at(Timestamp::from(self.now - minutes_ago * 60))
            .sign_with_keys(keys)
            .expect("a demo event signs");
        self.events.push(event.clone());
        event
    }

    fn say(&mut self, who: &Keys, channel: Uuid, minutes_ago: u64, text: &str) -> Event {
        self.sign(who, message(channel, text, None, &[]), minutes_ago)
    }

    fn reply(&mut self, who: &Keys, channel: Uuid, root: &Event, ago: u64, text: &str) -> Event {
        self.sign(who, message(channel, text, Some(root.id), &[]), ago)
    }

    fn react(&mut self, who: &Keys, target: &Event, minutes_ago: u64, emoji: &str) {
        let builder = buzz_sdk::build_reaction(target.id, emoji).expect("a demo reaction");
        self.sign(who, builder, minutes_ago);
    }
}

/// A kind-9 message, in a thread under `root` when given.
fn message(channel: Uuid, text: &str, root: Option<EventId>, mentions: &[&str]) -> EventBuilder {
    let thread = root.map(|root| buzz_sdk::ThreadRef {
        root_event_id: root,
        parent_event_id: root,
    });
    buzz_sdk::build_message(channel, text, thread.as_ref(), mentions, false, &[], &[])
        .expect("a demo message")
}

/// Builds the world for a relay whose media is served at `origin`
/// (`http://127.0.0.1:<port>`).
pub fn build(origin: &str) -> World {
    let now = Timestamp::now().as_secs();
    let relay_keys = seeded(0x51);
    let viewer = Keys::generate();
    let cast = Cast::new();
    let mut pen = Pen {
        now,
        events: Vec::new(),
    };
    let mut media = HashMap::new();
    let mut picture = |name: &str| {
        let bytes = AVATARS
            .iter()
            .find(|(avatar, _)| *avatar == name)
            .map(|(_, bytes)| *bytes)
            .unwrap_or_default();
        let file = format!("{}.jpg", hex::encode(sha2::Sha256::digest(bytes)));
        media.insert(file.clone(), bytes.to_vec());
        format!("{origin}/media/{file}")
    };
    let people = [
        (&cast.ada, "Ada", "Backend · a generated demo person", "ada"),
        (&cast.lin, "Lin", "Design · a generated demo person", "lin"),
        (&cast.sam, "Sam", "Infra · a generated demo person", "sam"),
        (&viewer, "You", "You, in the demo", "you"),
    ];
    for (keys, name, about, avatar) in people {
        let mut content =
            serde_json::to_value(Metadata::new().name(name).display_name(name).about(about))
                .unwrap_or_default();
        content["picture"] = picture(avatar).into();
        pen.sign(
            keys,
            EventBuilder::new(Kind::Metadata, content.to_string()),
            7 * 24 * 60,
        );
    }
    // Scout is an agent: its profile carries Ada's NIP-OA proof, it is a
    // `bot` member of the channels, and Ada's public policy lets anyone in
    // the team ask it something. bzz then marks it `◆` and shows it typing.
    let scout = cast.scout.public_key();
    let auth = buzz_sdk::nip_oa::compute_auth_tag(&cast.ada, &scout, "")
        .ok()
        .and_then(|auth| serde_json::from_str::<Vec<String>>(&auth).ok())
        .and_then(|auth| Tag::parse(auth).ok())
        .expect("Ada authorizes Scout");
    let mut content = serde_json::to_value(
        Metadata::new()
            .name("Scout")
            .display_name("Scout")
            .about("Ada's agent · generated, with scripted answers"),
    )
    .unwrap_or_default();
    content["picture"] = picture("scout").into();
    pen.sign(
        &cast.scout,
        EventBuilder::new(Kind::Metadata, content.to_string()).tags([auth]),
        7 * 24 * 60,
    );
    pen.sign(
        &cast.scout,
        EventBuilder::new(
            Kind::Custom(10_100),
            json!({"name": "Scout", "capabilities": ["release notes", "pull request review"]})
                .to_string(),
        ),
        7 * 24 * 60,
    );
    pen.sign(
        &cast.ada,
        EventBuilder::new(
            Kind::Custom(30_177),
            json!({"name": "Scout", "respond_to": "anyone"}).to_string(),
        )
        .tags([tag(&["d", &scout.to_hex()])]),
        7 * 24 * 60,
    );

    let team = channel_id("team");
    let design = channel_id("design");
    let infra = channel_id("infra");
    let launch = channel_id("launch");
    let dm = channel_id("dm-lin");
    let people = [&viewer, &cast.ada, &cast.lin, &cast.sam];
    let relay = |pen: &mut Pen, tags: Vec<Tag>, kind: u16| {
        pen.sign(
            &relay_keys,
            EventBuilder::new(Kind::Custom(kind), "").tags(tags),
            3 * 24 * 60,
        );
    };
    for (id, name, about) in [
        (team, "team", "Harbor 2.4 ships Thursday · a generated demo"),
        (
            design,
            "design",
            "Screens, words and icons · a generated demo",
        ),
        (infra, "infra", "Deploys and on-call · a generated demo"),
        (launch, "launch", "The launch-day plan · a generated demo"),
    ] {
        let id = id.to_string();
        relay(
            &mut pen,
            vec![
                tag(&["d", &id]),
                tag(&["name", name]),
                tag(&["about", about]),
                tag(&["t", "stream"]),
            ],
            39_000,
        );
        let mut members = vec![tag(&["d", &id])];
        for (index, who) in people.iter().enumerate() {
            let role = if index == 1 { "owner" } else { "member" };
            members.push(tag(&["p", &who.public_key().to_hex(), "", role]));
        }
        members.push(tag(&["p", &scout.to_hex(), "", "bot"]));
        relay(&mut pen, members, 39_002);
    }
    {
        let id = dm.to_string();
        let members = [&viewer, &cast.lin];
        let mut tags = vec![
            tag(&["d", &id]),
            tag(&["name", "Lin"]),
            tag(&["t", "dm"]),
            tag(&["private"]),
            tag(&["hidden"]),
        ];
        tags.extend(
            members
                .iter()
                .map(|who| tag(&["p", &who.public_key().to_hex()])),
        );
        relay(&mut pen, tags, 39_000);
        let mut tags = vec![tag(&["d", &id])];
        tags.extend(
            members
                .iter()
                .map(|who| tag(&["p", &who.public_key().to_hex(), "", "member"])),
        );
        relay(&mut pen, tags, 39_002);
    }

    // #infra, #design and #launch: a little each.
    pen.say(
        &cast.sam,
        infra,
        190,
        "Staging is on 2.4-rc2. Error rate flat for 12 hours.",
    );
    pen.say(&cast.ada, infra, 185, "Nice. Backups ran on schedule too.");
    pen.say(
        &cast.sam,
        infra,
        60,
        "The rollback plan is step 7 of the runbook. It takes about four minutes.",
    );
    pen.say(
        &cast.sam,
        infra,
        17,
        "Starting a huddle to walk through the deploy, join if you like.",
    );
    pen.say(
        &cast.lin,
        design,
        240,
        "The new empty state for the incidents page: short words, one button.",
    );
    pen.say(&cast.ada, design, 230, "Reads well. Ship it.");
    pen.say(
        &cast.lin,
        design,
        95,
        "Dark theme contrast checked: every text passes AA now.",
    );
    pen.say(
        &cast.ada,
        launch,
        300,
        "Launch is Thursday 15:00 UTC. The checklist lives in #team.",
    );
    pen.say(
        &cast.lin,
        launch,
        290,
        "The blog post draft is ready for review.",
    );

    // #team: the first screen.
    pen.say(
        &cast.ada,
        team,
        150,
        "Morning! 2.4-rc2 is on staging, CI green on all four platforms.",
    );
    pen.say(
        &cast.lin,
        team,
        140,
        "I'm polishing the release notes today. Screenshots by noon.",
    );
    pen.say(
        &cast.sam,
        team,
        130,
        "Load test passed: 4k checks a minute, p99 under 80 ms.",
    );
    let checklist = pen.say(
        &cast.ada,
        team,
        120,
        "Release checklist for Thursday: anything still blocking?",
    );
    pen.reply(
        &cast.lin,
        team,
        &checklist,
        118,
        "Copy is final. The changelog has one TODO left.",
    );
    pen.reply(
        &cast.sam,
        team,
        &checklist,
        116,
        "The status page migration ran twice on staging, clean both times.",
    );
    pen.reply(
        &cast.scout,
        team,
        &checklist,
        115,
        "I read the 14 pull requests merged since 2.3: two have no changelog line (#412 and #418). I drafted both in the release notes.",
    );
    pen.reply(
        &cast.ada,
        team,
        &checklist,
        110,
        "Thanks Scout. Then we ship 🎉",
    );
    pen.react(&cast.lin, &checklist, 117, "🚀");
    pen.react(&cast.sam, &checklist, 116, "🚀");
    pen.react(&cast.ada, &checklist, 109, "👍");
    let snippet = pen.say(
        &cast.sam,
        team,
        70,
        "For whoever is on call Thursday:\n```sh\nharbor deploy --env prod --version 2.4.0\nharbor status --watch\n```",
    );
    pen.react(&cast.ada, &snippet, 69, "🙏");
    pen.say(
        &cast.lin,
        team,
        40,
        "Screenshots are in. The dark theme looks great on a big screen.",
    );
    let viewer_hex = viewer.public_key().to_hex();
    pen.sign(
        &cast.ada,
        message(team, WELCOME, None, &[viewer_hex.as_str()]),
        2,
    );

    // A huddle in #infra: Sam started it, Sam, Ada and Lin are in, and You
    // listen as the demo starts (its lanes are generated voices, and your
    // microphone is never opened). Joins are
    // signed by the relay, as Buzz does it.
    let backing = channel_id("huddle-infra").to_string();
    let lifecycle = |kind: u16| {
        EventBuilder::new(
            Kind::Custom(kind),
            json!({ "ephemeral_channel_id": backing }).to_string(),
        )
        .tags([tag(&["h", &infra.to_string()])])
    };
    pen.sign(&cast.sam, lifecycle(48_100), 16);
    for (who, minutes_ago) in [
        (&cast.sam, 16),
        (&cast.ada, 15),
        (&cast.lin, 12),
        (&viewer, 0),
    ] {
        let joined = lifecycle(48_101).tags([tag(&["p", &who.public_key().to_hex()])]);
        pen.sign(&relay_keys, joined, minutes_ago);
    }

    // The DM with Lin, unread.
    pen.say(
        &cast.lin,
        dm,
        9,
        "Got a minute after standup? I'd love a second pair of eyes on the release post.",
    );

    let script = Script {
        team,
        dm,
        viewer: viewer.public_key(),
        cast,
        said: 0,
    };
    World {
        events: pen.events,
        media,
        relay_keys,
        viewer,
        team,
        script,
    }
}

/// Ada's welcome, the newest message of `#team`: what this is, and three
/// things to try.
pub const WELCOME: &str = "@You welcome to the team! Everything here is generated and stays in this terminal. Three things to try:\n1. `k` up to my checklist, then `Enter` opens its thread\n2. `/` searches: try rollback\n3. `i` writes a message: mention @Scout and our agent answers";

/// How long Scout "types" before it answers, and a teammate's pause.
const SCOUT_TYPES_AFTER: Duration = Duration::from_millis(500);
const SCOUT_ANSWERS_AFTER: Duration = Duration::from_millis(2_500);
const TEAMMATE_ANSWERS_AFTER: Duration = Duration::from_millis(1_500);

/// The scripted answers: a teammate where the viewer writes, Lin in the DM,
/// and Scout in a thread whenever it is mentioned. A few, then the team
/// goes quiet; Scout always answers.
pub struct Script {
    team: Uuid,
    dm: Uuid,
    viewer: PublicKey,
    cast: Cast,
    said: usize,
}

impl Script {
    /// What the world answers to `event`, each after its delay. The events
    /// are signed when they are due, so their time is the time they arrive.
    pub fn answer(&mut self, event: &Event) -> Vec<(Duration, Reply)> {
        if event.kind.as_u16() != 9 || event.pubkey != self.viewer {
            return Vec::new();
        }
        let Some(channel) = first(event, "h").and_then(|h| Uuid::parse_str(&h).ok()) else {
            return Vec::new();
        };
        let root = thread_root(event);
        let scout = self.cast.scout.public_key().to_hex();
        let mentions_scout =
            values(event, "p").contains(&scout) || event.content.to_lowercase().contains("@scout");
        if mentions_scout && channel != self.dm {
            // In the thread of the message that asked, or the one it is in.
            let root = root.unwrap_or(event.id);
            // It types where the viewer looks: the channel, and the thread
            // it answers in, in case that is open.
            // A typing line lasts 8 s and only a message in the same place
            // ends it, so the channel's is dated to end as the answer comes.
            let typing_for = SCOUT_ANSWERS_AFTER - SCOUT_TYPES_AFTER;
            let channel_typing =
                Reply::typing(&self.cast.scout, channel, None).ending_after(typing_for);
            return vec![
                (SCOUT_TYPES_AFTER, channel_typing),
                (
                    SCOUT_TYPES_AFTER,
                    Reply::typing(&self.cast.scout, channel, Some((root, event.id))),
                ),
                (
                    SCOUT_ANSWERS_AFTER,
                    Reply::message(&self.cast.scout, channel, SCOUT_ANSWER, Some(root)),
                ),
            ];
        }
        if self.said >= 4 {
            return Vec::new();
        }
        self.said += 1;
        let (who, text) = if channel == self.dm {
            (&self.cast.lin, LIN_IN_THE_DM)
        } else if channel == self.team {
            match self.said {
                1 => (&self.cast.ada, ADA_ANSWERS),
                2 => (&self.cast.sam, SAM_ANSWERS),
                _ => (&self.cast.lin, LIN_ANSWERS),
            }
        } else {
            (&self.cast.sam, SAM_ELSEWHERE)
        };
        vec![(
            TEAMMATE_ANSWERS_AFTER,
            Reply::message(who, channel, text, root),
        )]
    }
}

pub const SCOUT_ANSWER: &str = "Hi, I'm Scout, Ada's agent, with scripted answers in this demo. In your own community an agent like me runs where its owner says, answers in a thread like this one, and does only what it is allowed. You can make one with bzz agent new.";
const ADA_ANSWERS: &str = "Welcome aboard 👋 What you write stays here until you leave the demo.";
const SAM_ANSWERS: &str = "Try :theme to change the colours, or Space for more.";
const LIN_ANSWERS: &str = "Mention @Scout and watch the thread.";
const LIN_IN_THE_DM: &str =
    "Thanks! I'm a generated teammate: nothing you write here leaves this terminal.";
const SAM_ELSEWHERE: &str = "Seen 👍 (a scripted answer: the demo has no network.)";

/// One answer of the script, signed when it is due.
pub struct Reply {
    keys: Keys,
    builder: EventBuilder,
    /// Dated this much earlier than it is sent.
    earlier: Duration,
}

impl Reply {
    fn message(keys: &Keys, channel: Uuid, text: &str, root: Option<EventId>) -> Self {
        Self {
            keys: keys.clone(),
            builder: message(channel, text, root, &[]),
            earlier: Duration::ZERO,
        }
    }

    /// The agent's typing line, in the channel or in a thread.
    fn typing(keys: &Keys, channel: Uuid, thread: Option<(EventId, EventId)>) -> Self {
        let thread = thread.map(|(root, parent)| (root.to_hex(), parent.to_hex()));
        let builder = crate::agents::typing::build(
            channel,
            thread
                .as_ref()
                .map(|(root, parent)| (root.as_str(), parent.as_str())),
        )
        .unwrap_or_else(|| EventBuilder::new(Kind::Custom(20_002), ""));
        Self {
            keys: keys.clone(),
            builder,
            earlier: Duration::ZERO,
        }
    }

    /// A typing line that ends `after` it is sent rather than after
    /// bzz's usual 8 s.
    fn ending_after(mut self, after: Duration) -> Self {
        let ttl = Duration::from_secs(crate::agents::typing::TYPING_TTL_SECONDS);
        self.earlier = ttl.saturating_sub(after);
        self
    }

    pub fn sign(self) -> Option<Event> {
        let now = Timestamp::now().as_secs();
        self.builder
            .custom_created_at(Timestamp::from(now.saturating_sub(self.earlier.as_secs())))
            .sign_with_keys(&self.keys)
            .ok()
    }
}

/// The root of the thread `event` is in, if it is in one.
fn thread_root(event: &Event) -> Option<EventId> {
    let mut root = None;
    let mut reply = None;
    for tag in event.tags.iter() {
        let values = tag.as_slice();
        if values.first().map(String::as_str) != Some("e") {
            continue;
        }
        let id = values.get(1).and_then(|id| EventId::from_hex(id).ok());
        match values.get(3).map(String::as_str) {
            Some("root") => root = id,
            Some("reply") => reply = id,
            _ => {}
        }
    }
    root.or(reply)
}

pub fn first(event: &Event, name: &str) -> Option<String> {
    values(event, name).into_iter().next()
}

pub fn values(event: &Event, name: &str) -> Vec<String> {
    event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().map(String::as_str) == Some(name))
        .filter_map(|tag| tag.as_slice().get(1).cloned())
        .collect()
}
