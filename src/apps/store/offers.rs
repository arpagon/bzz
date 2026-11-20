//! An app offered in a channel, taken in as a version that waits (plan
//! v0.16.0 M6; rule 16).
//!
//! A head of type `bzz.app` reaches this bzz on the offers' own sync
//! (`service::offers`). It is taken in in two steps, because the second
//! runs the script apart and may take seconds:
//!
//! 1. [`Apps::offer_arrived`], on the store's thread, quick: a revision
//!    seen before is nothing; earlier revisions of the same artifact that
//!    still wait are let go (an offer is the head revision and its signer);
//!    a deletion withdraws; what [`share::read`] refused is written down
//!    with the reason; an offer in a direct message, bytes the owner refused
//!    before, bytes that already run, and a person or a channel with too
//!    many offers waiting are answered here. Anything else is to be
//!    examined: [`Step::Examine`].
//! 2. [`examine`](super::examine) off the store's thread, then
//!    [`Apps::offer_examined`]: the version is recorded as every proposed
//!    version is ([`Apps::record`]), from [`Proposer::member`] in that
//!    channel. One that does not load is recorded as failed and never
//!    offered; one that loads **waits**. Nothing of it runs: only
//!    [`Apps::runnable`] hands out a script, and only an approved one.
//!
//! What happened is answered as [`Told`], for the owner to be told.

use std::collections::HashSet;

use uuid::Uuid;

use super::{AppError, AppResult, Apps, Examined, Recorded, Submission, now};
use crate::apps::share::{Head, Offer, Refusal};
use crate::protocol::artifact::Artifact;
use crate::store::app_offers::{OfferRow, OfferState};
use crate::store::apps::{Proposer, VersionState};

/// Offers of one person that may wait for the owner's answer at once, in
/// a community. A person who offers more is told to wait.
pub const OFFERS_PER_PERSON: usize = 3;
/// Offers that may wait in one channel at once, from everyone.
pub const OFFERS_PER_CHANNEL: usize = 8;

/// A head of type `bzz.app`, as it reached this bzz in a channel.
#[derive(Clone, Debug)]
pub struct Arrival {
    pub channel: Uuid,
    /// The channel is a direct message: no app is offered there.
    pub dm: bool,
    pub artifact: Uuid,
    /// The event id of the revision.
    pub revision: String,
    /// Who signed the revision.
    pub signer: String,
    pub read: Result<Head, Refusal>,
}

impl Arrival {
    pub fn of(head: &Artifact, channel: Uuid, dm: bool) -> Self {
        Self {
            channel,
            dm,
            artifact: head.id,
            revision: head.head.clone(),
            signer: head.author.clone(),
            read: crate::apps::share::read(head, channel),
        }
    }
}

/// What the owner is told of an offer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Told {
    /// It waits for his answer as a version of the app.
    Offered {
        name: String,
        version: u32,
        from: Proposer,
        /// Who signed the revision it took the place of, when that was
        /// another person.
        replaces: Option<Proposer>,
        /// Who the installed version came from, when one runs.
        installed_from: Option<Proposer>,
        /// Its record types are not those of the version that runs.
        types_changed: bool,
    },
    /// Its bytes are those that run here: nothing to approve.
    Installed { name: String, from: Proposer },
    /// The owner refused these exact bytes before; not offered again.
    Declined { name: String, from: Proposer },
    /// It did not load. It is recorded as failed and never offered.
    Failed {
        name: String,
        from: Proposer,
        error: String,
    },
    /// bzz did not take it in.
    Refused {
        name: Option<String>,
        from: Proposer,
        why: String,
    },
    /// An offer that waited is gone: deleted in the channel (`by` who
    /// signed the deletion), or no longer in the channel's set (`by` is
    /// None). An installed copy stays.
    Withdrawn {
        name: String,
        version: u32,
        signer: Proposer,
        by: Option<Proposer>,
    },
}

/// The first step's answer.
#[derive(Debug)]
pub enum Step {
    Done(Vec<Told>),
    /// The script is to be loaded apart (`examine`), then given to
    /// [`Apps::offer_examined`] with this.
    Examine(Box<ToExamine>),
}

/// An offer that passed the first step.
#[derive(Clone, Debug)]
pub struct ToExamine {
    pub submission: Submission,
    pub told: Vec<Told>,
    community: Uuid,
    row: OfferRow,
    from: Proposer,
    replaces: Option<Proposer>,
}

fn proposer(apps: &Apps<'_>, community: Uuid, key: &str, channel: Uuid) -> Proposer {
    let name = apps
        .store
        .profile(community, key)
        .ok()
        .flatten()
        .map_or_else(
            || crate::domain::abbreviated_pubkey(key),
            |profile| profile.label(),
        );
    Proposer::member(key, name).in_channel(channel)
}

impl Apps<'_> {
    /// The first step of taking an offer in. `me` is this bzz's identity:
    /// its own offers are not offered to itself.
    pub fn offer_arrived(
        &mut self,
        community: Uuid,
        me: &str,
        arrival: Arrival,
    ) -> AppResult<Step> {
        if self
            .store
            .app_offer(community, &arrival.revision)?
            .is_some()
        {
            return Ok(Step::Done(Vec::new()));
        }
        let from = proposer(self, community, &arrival.signer, arrival.channel);
        let withdrawn_by = match &arrival.read {
            Ok(Head::Withdrawn { .. }) => Some(from.clone()),
            _ => None,
        };
        // An offer is the head revision: every earlier one that waits is
        // let go, whatever this one turns out to be.
        let mut told = Vec::new();
        let mut replaces = None;
        for earlier in self
            .store
            .app_offers_of_artifact(community, arrival.artifact)?
        {
            if earlier.state != OfferState::Waiting || earlier.revision == arrival.revision {
                continue;
            }
            let signer = proposer(self, community, &earlier.signer, earlier.channel);
            let (state, detail) = match &withdrawn_by {
                Some(by) => (
                    OfferState::Withdrawn,
                    format!(
                        "withdrawn in the channel by {}",
                        by.name.clone().unwrap_or_default()
                    ),
                ),
                None => (
                    OfferState::Replaced,
                    format!(
                        "replaced in the channel by a revision signed by {}",
                        from.name.clone().unwrap_or_default()
                    ),
                ),
            };
            if let Some((name, version)) = self.let_go(&earlier, state, &detail)? {
                if withdrawn_by.is_some() {
                    told.push(Told::Withdrawn {
                        name,
                        version,
                        signer: signer.clone(),
                        by: withdrawn_by.clone(),
                    });
                } else if !signer.is(&from) {
                    replaces = Some(signer);
                }
            }
        }
        let mut row = OfferRow {
            community,
            revision: arrival.revision.clone(),
            channel: arrival.channel,
            artifact: arrival.artifact,
            signer: arrival.signer.clone(),
            name: None,
            script_sha256: None,
            brief_sha256: None,
            version: None,
            state: OfferState::Refused,
            detail: None,
            received_at: now(),
        };
        let offer = match arrival.read {
            Ok(Head::Withdrawn { .. }) => {
                row.state = OfferState::Withdrawn;
                self.store.app_offer_insert(&row)?;
                return Ok(Step::Done(told));
            }
            Err(refusal) => {
                let why = refusal.to_string();
                row.detail = Some(why.clone());
                self.store.app_offer_insert(&row)?;
                told.push(Told::Refused {
                    name: None,
                    from,
                    why,
                });
                return Ok(Step::Done(told));
            }
            Ok(Head::Offer(offer)) => offer,
        };
        let Offer {
            manifest,
            script,
            brief,
            script_sha256,
            ..
        } = *offer;
        let name = manifest.name.clone();
        let brief_sha256 = super::manifest::sha256(brief.as_bytes());
        row.name = Some(name.clone());
        row.script_sha256 = Some(script_sha256.clone());
        row.brief_sha256 = Some(brief_sha256.clone());
        // This bzz's own revision: shared from here, offered to nobody here.
        if arrival.signer == me {
            row.state = OfferState::Installed;
            row.detail = Some("shared from this bzz".into());
            self.store.app_offer_insert(&row)?;
            return Ok(Step::Done(told));
        }
        let mut refuse = |apps: &mut Self, state: OfferState, why: String| -> AppResult<()> {
            row.state = state;
            row.detail = Some(why);
            apps.store.app_offer_insert(&row)?;
            Ok(())
        };
        if arrival.dm {
            let why = "an app is not offered in a direct message, only in a channel".to_owned();
            refuse(self, OfferState::Refused, why.clone())?;
            told.push(Told::Refused {
                name: Some(name),
                from,
                why,
            });
            return Ok(Step::Done(told));
        }
        if self
            .store
            .app_bytes_declined(community, &name, &script_sha256, &brief_sha256)?
        {
            refuse(
                self,
                OfferState::Declined,
                "you refused these exact bytes before".into(),
            )?;
            told.push(Told::Declined { name, from });
            return Ok(Step::Done(told));
        }
        if let Some(app) = self.store.app(community, &name)?
            && let Some(current) = app.current
            && let Some(running) = self.store.app_version(community, &name, current)?
            && running.state == VersionState::Approved
            && running.script_sha256 == script_sha256
            && running.brief_sha256 == brief_sha256
        {
            refuse(
                self,
                OfferState::Installed,
                format!("the bytes of version {current}, which runs here"),
            )?;
            told.push(Told::Installed { name, from });
            return Ok(Step::Done(told));
        }
        // Not buried in offers: a bound for each person and each channel.
        let waiting = self.store.app_offers_waiting(community, None)?;
        let theirs = waiting
            .iter()
            .filter(|other| {
                other.signer == arrival.signer && other.name.as_deref() != Some(name.as_str())
            })
            .count();
        let here = waiting
            .iter()
            .filter(|other| {
                other.channel == arrival.channel
                    && !(other.signer == arrival.signer
                        && other.name.as_deref() == Some(name.as_str()))
            })
            .count();
        let crowded = if theirs >= OFFERS_PER_PERSON {
            Some(format!(
                "{OFFERS_PER_PERSON} other offers of this person wait for your answer; answer them first"
            ))
        } else if here >= OFFERS_PER_CHANNEL {
            Some(format!(
                "{OFFERS_PER_CHANNEL} offers wait for your answer in this channel; answer them first"
            ))
        } else {
            None
        };
        if let Some(why) = crowded {
            refuse(self, OfferState::Refused, why.clone())?;
            told.push(Told::Refused {
                name: Some(name),
                from,
                why,
            });
            return Ok(Step::Done(told));
        }
        let mut submission =
            Submission::new(name.clone(), script, brief.into_bytes(), from.clone())
                .titled(manifest.title.clone());
        submission.declared = Some(manifest);
        Ok(Step::Examine(Box::new(ToExamine {
            submission,
            told,
            community,
            row,
            from,
            replaces,
        })))
    }

    /// The second step: the examined version is recorded, and the revision
    /// with it.
    pub fn offer_examined(
        &mut self,
        examine: ToExamine,
        examined: Examined,
    ) -> AppResult<Vec<Told>> {
        let ToExamine {
            mut told,
            community,
            mut row,
            from,
            replaces,
            ..
        } = examine;
        let name = examined.name().to_owned();
        let running = self
            .store
            .app(community, &name)?
            .and_then(|app| app.current)
            .map(|current| self.store.app_version(community, &name, current))
            .transpose()?
            .flatten();
        let types = examined.types().map(<[String]>::to_vec);
        let recorded = match self.record(community, examined) {
            Ok(recorded) => recorded,
            Err(AppError::TooManyWaiting(_)) => {
                let why = format!(
                    "{} versions of {name} wait for your answer; answer one first",
                    super::WAITING_AT_ONCE
                );
                row.detail = Some(why.clone());
                self.store.app_offer_insert(&row)?;
                told.push(Told::Refused {
                    name: Some(name),
                    from,
                    why,
                });
                return Ok(told);
            }
            Err(error) => return Err(error),
        };
        match recorded {
            Recorded::Waiting { version, .. } => {
                row.state = OfferState::Waiting;
                row.version = Some(version);
                self.store.app_offer_insert(&row)?;
                let types_changed = match (&running, &types) {
                    (Some(running), Some(types)) => {
                        let (mut old, mut new) = (running.types.clone(), types.clone());
                        old.sort();
                        new.sort();
                        old != new
                    }
                    _ => false,
                };
                told.push(Told::Offered {
                    name,
                    version,
                    from,
                    replaces,
                    installed_from: running.map(|running| running.proposer),
                    types_changed,
                });
            }
            Recorded::Failed { version, error } => {
                row.state = OfferState::Failed;
                row.version = Some(version);
                row.detail = Some(error.clone());
                self.store.app_offer_insert(&row)?;
                told.push(Told::Failed { name, from, error });
            }
            Recorded::Running { version } => {
                row.state = OfferState::Installed;
                row.detail = Some(format!("the bytes of version {version}, which runs here"));
                self.store.app_offer_insert(&row)?;
                told.push(Told::Installed { name, from });
            }
        }
        Ok(told)
    }

    /// A complete set of the offers of a channel arrived: every offer that
    /// waits and whose revision is not in it is no longer offered there.
    pub fn offers_in_set(
        &mut self,
        community: Uuid,
        channel: Uuid,
        heads: &HashSet<String>,
    ) -> AppResult<Vec<Told>> {
        let mut told = Vec::new();
        for row in self.store.app_offers_waiting(community, Some(channel))? {
            if heads.contains(&row.revision) {
                continue;
            }
            let signer = proposer(self, community, &row.signer, channel);
            if let Some((name, version)) = self.let_go(
                &row,
                OfferState::Withdrawn,
                "no longer offered in the channel",
            )? {
                told.push(Told::Withdrawn {
                    name,
                    version,
                    signer,
                    by: None,
                });
            }
        }
        Ok(told)
    }

    /// The offers that wait for an answer in a community, oldest first.
    pub fn offers_waiting(&self, community: Uuid) -> AppResult<Vec<OfferRow>> {
        Ok(self.store.app_offers_waiting(community, None)?)
    }

    /// Lets go of an offer that waited: its version no longer waits, and an
    /// app that never ran and has nothing else waiting is not kept. Answers
    /// the app and the version, when one was let go.
    fn let_go(
        &mut self,
        row: &OfferRow,
        state: OfferState,
        detail: &str,
    ) -> AppResult<Option<(String, u32)>> {
        self.store
            .app_offer_set_state(row.community, &row.revision, state, Some(detail))?;
        let (Some(name), Some(version)) = (row.name.clone(), row.version) else {
            return Ok(None);
        };
        if !self
            .store
            .app_withdraw_version(row.community, &name, version, detail, now())?
        {
            return Ok(None);
        }
        self.tidy(row.community, &name)?;
        if let Some(app) = self.store.app(row.community, &name)?
            && app.current.is_none()
            && !self
                .store
                .app_versions(row.community, &name)?
                .iter()
                .any(|version| version.state == VersionState::Waiting)
        {
            // Offered, never approved, and now nothing: installed nowhere.
            self.remove(row.community, &name)?;
        }
        Ok(Some((name, version)))
    }
}

#[cfg(test)]
mod tests {
    //! The proofs of M6 at the store: an offer waits, runs only after its
    //! approval, shows who signed it, and goes when it is withdrawn.

    use nostr::Keys;
    use tempfile::TempDir;

    use super::*;
    use crate::apps::runtime::Limits;
    use crate::apps::share;
    use crate::apps::store::manifest::Manifest;
    use crate::apps::store::review::TypesChange;
    use crate::apps::store::{AppError, Bundle, examine_with};
    use crate::config::{Config, IdentityConfig, KeyBackend};
    use crate::protocol::artifact::parse;
    use crate::store::Store;

    struct Fixture {
        _temporary: TempDir,
        store: Store,
        root: std::path::PathBuf,
        community: Uuid,
        me: Keys,
        channel: Uuid,
    }

    impl Fixture {
        fn new() -> Self {
            let temporary = TempDir::new().unwrap();
            let me = Keys::generate();
            let identity = IdentityConfig {
                id: Uuid::new_v4(),
                label: "owner".into(),
                pubkey: me.public_key().to_hex(),
                backend: KeyBackend::EncryptedFile,
                key_ref: "owner".into(),
            };
            let mut config = Config::default();
            config.identities.push(identity.clone());
            let community = config
                .add_community("c".into(), "ws://localhost:3030".into(), identity.id, true)
                .unwrap();
            let mut store = Store::open(temporary.path().join("bzz.db")).unwrap();
            store.sync_config(&config).unwrap();
            Self {
                root: temporary.path().join("apps"),
                _temporary: temporary,
                store,
                community,
                me,
                channel: Uuid::new_v4(),
            }
        }

        fn apps(&mut self) -> Apps<'_> {
            Apps::at(&mut self.store, self.root.clone())
        }

        /// A head as it arrives, taken in through both steps.
        fn arrive(&mut self, head: &Artifact, dm: bool) -> Vec<Told> {
            let (community, channel, me) =
                (self.community, self.channel, self.me.public_key().to_hex());
            let step = self
                .apps()
                .offer_arrived(community, &me, Arrival::of(head, channel, dm))
                .unwrap();
            match step {
                Step::Done(told) => told,
                Step::Examine(examine) => {
                    let examined =
                        examine_with(examine.submission.clone(), &Limits::patient()).unwrap();
                    self.apps().offer_examined(*examine, examined).unwrap()
                }
            }
        }

        fn waiting(&mut self) -> Vec<(String, u32)> {
            let community = self.community;
            self.apps()
                .waiting(community)
                .unwrap()
                .into_iter()
                .map(|version| (version.name, version.version))
                .collect()
        }
    }

    fn script(types: &[&str], says: &str) -> String {
        let types = types
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "local app = {{}}\napp.api = 1\napp.types = {{ {types} }}\nfunction app.view(state, ctx)\n  return ui.text {{ \"{says}\" }}\nend\nreturn app\n"
        )
    }

    fn bundle(name: &str, types: &[&str], says: &str) -> Bundle {
        let manifest = Manifest {
            name: name.into(),
            title: crate::apps::store::manifest::title_from_name(name),
            api: 1,
            types: types.iter().map(|name| (*name).to_owned()).collect(),
        };
        Bundle {
            script: script(types, says).into_bytes(),
            manifest_text: manifest.to_text(),
            manifest,
            brief: format!("The app {name}, which says {says}.\n"),
        }
    }

    fn created(keys: &Keys, channel: Uuid, bundle: &Bundle) -> Artifact {
        parse(
            &share::create(channel, Uuid::new_v4(), bundle)
                .unwrap()
                .sign_with_keys(keys)
                .unwrap(),
        )
        .unwrap()
    }

    fn revised(keys: &Keys, head: &Artifact, bundle: &Bundle) -> Artifact {
        parse(
            &share::revise(head, bundle)
                .unwrap()
                .sign_with_keys(keys)
                .unwrap(),
        )
        .unwrap()
    }

    fn withdrawn(keys: &Keys, head: &Artifact) -> Artifact {
        parse(&share::withdraw(head).unwrap().sign_with_keys(keys).unwrap()).unwrap()
    }

    fn offered(told: &[Told]) -> (String, u32, Proposer) {
        match told {
            [
                ..,
                Told::Offered {
                    name,
                    version,
                    from,
                    ..
                },
            ] => (name.clone(), *version, from.clone()),
            other => panic!("not offered: {other:?}"),
        }
    }

    /// Gate 2 and 1 at the store: an offer is a version that waits. The
    /// one function that hands out a script to run gives nothing of it
    /// before the approval, and exactly its bytes after; approving opens it
    /// in the channel it was offered in.
    #[test]
    fn an_offer_waits_and_nothing_of_it_runs_before_its_approval() {
        let mut fixture = Fixture::new();
        let (community, channel) = (fixture.community, fixture.channel);
        let ana = Keys::generate();
        let board = bundle("board", &["card"], "one");
        let head = created(&ana, channel, &board);
        let told = fixture.arrive(&head, false);
        let (name, version, from) = offered(&told);
        assert_eq!((name.as_str(), version), ("board", 1));
        assert_eq!(from.kind, crate::store::apps::ProposerKind::Member);
        assert_eq!(from.id.as_deref(), Some(ana.public_key().to_hex().as_str()));
        assert_eq!(from.channel, Some(channel));

        // Nothing of it runs.
        let refused = fixture.apps().runnable(community, "board").unwrap_err();
        assert!(
            matches!(refused, AppError::NothingApproved { waiting: 1, .. }),
            "{refused}"
        );
        // What the owner is shown: who signed this exact revision, by key.
        let review = fixture.apps().review(community, "board", 1).unwrap();
        let text = review.render(|_| "then".into());
        assert!(
            text.contains(&format!(
                "a person of the channel ({})",
                ana.public_key().to_hex()
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!("this exact revision ({}…)", &head.head[..16])),
            "{text}"
        );
        assert_eq!(review.types_change, TypesChange::First);
        assert!(text.contains("The app board, which says one."), "{text}");

        // The same revision again (a resync) is nothing new.
        assert!(fixture.arrive(&head, false).is_empty());
        assert_eq!(fixture.waiting(), [("board".to_owned(), 1)]);

        let approved = fixture
            .apps()
            .approve(community, "board", 1, &review.hash)
            .unwrap();
        assert_eq!(approved.opened_in, Some(channel));
        let runnable = fixture.apps().runnable(community, "board").unwrap();
        assert_eq!(runnable.script(), board.script.as_slice());
        assert_eq!(
            fixture.apps().get(community, "board").unwrap().channels,
            [channel]
        );
        let offers = fixture.apps().offers_waiting(community).unwrap();
        assert!(offers.is_empty(), "{offers:?}");
    }

    /// Gate 3, and the source of a later version: a new revision is a new
    /// offer with its difference; a changed type list is marked; a revision
    /// signed by another member is that member's, plainly.
    #[test]
    fn a_later_version_is_a_new_offer_and_a_changed_type_list_or_source_is_marked() {
        let mut fixture = Fixture::new();
        let (community, channel) = (fixture.community, fixture.channel);
        let (ana, bob) = (Keys::generate(), Keys::generate());
        let head = created(&ana, channel, &bundle("board", &["card"], "one"));
        let (_, first, _) = offered(&fixture.arrive(&head, false));
        let hash = fixture
            .apps()
            .review(community, "board", first)
            .unwrap()
            .hash;
        fixture
            .apps()
            .approve(community, "board", first, &hash)
            .unwrap();

        // Ana's later version, with a type more: never applied by itself.
        let later = revised(&ana, &head, &bundle("board", &["card", "tag"], "two"));
        let told = fixture.arrive(&later, false);
        let Some(Told::Offered {
            version,
            types_changed,
            installed_from,
            replaces,
            ..
        }) = told.last().cloned()
        else {
            panic!("{told:?}")
        };
        assert!(types_changed && replaces.is_none());
        assert_eq!(
            installed_from.and_then(|from| from.id),
            Some(ana.public_key().to_hex())
        );
        let review = fixture.apps().review(community, "board", version).unwrap();
        assert!(review.types_change.is_changed());
        assert!(review.another_source().is_none(), "the same person");
        let text = review.render(|_| "then".into());
        assert!(
            text.contains("CHANGED from the version that runs:"),
            "{text}"
        );
        assert!(text.contains("+ tag"), "{text}");
        assert!(text.contains("+  return ui.text { \"two\" }"), "{text}");
        assert_eq!(
            fixture
                .apps()
                .runnable(community, "board")
                .unwrap()
                .version(),
            first,
            "the installed version runs until the owner answers"
        );

        // Bob publishes a revision of Ana's offer: it is Bob's offer now,
        // and Ana's waiting one is gone.
        let bobs = revised(&bob, &later, &bundle("board", &["card", "tag"], "bob"));
        let told = fixture.arrive(&bobs, false);
        let Some(Told::Offered {
            version: bobs_version,
            from,
            replaces,
            ..
        }) = told.last().cloned()
        else {
            panic!("{told:?}")
        };
        assert_eq!(from.id, Some(bob.public_key().to_hex()));
        assert_eq!(
            replaces.and_then(|was| was.id),
            Some(ana.public_key().to_hex())
        );
        assert_eq!(fixture.waiting(), [("board".to_owned(), bobs_version)]);
        let review = fixture
            .apps()
            .review(community, "board", bobs_version)
            .unwrap();
        assert_eq!(
            review.another_source().and_then(|was| was.id.clone()),
            Some(ana.public_key().to_hex())
        );
        let text = review.render(|_| "then".into());
        assert!(
            text.contains("Source:   CHANGED: the version installed here came from"),
            "{text}"
        );
        assert!(
            text.contains(&format!("({})", bob.public_key().to_hex())),
            "{text}"
        );
    }

    /// Gate 7: withdrawing removes the offer that waits and leaves an
    /// installed copy; a deletion by someone else is told as theirs; an app
    /// that was only offered is not kept.
    #[test]
    fn withdrawing_removes_the_waiting_offer_and_leaves_an_installed_copy() {
        let mut fixture = Fixture::new();
        let (community, channel) = (fixture.community, fixture.channel);
        let (ana, carol) = (Keys::generate(), Keys::generate());
        let head = created(&ana, channel, &bundle("board", &["card"], "one"));
        let (_, first, _) = offered(&fixture.arrive(&head, false));
        let hash = fixture
            .apps()
            .review(community, "board", first)
            .unwrap()
            .hash;
        fixture
            .apps()
            .approve(community, "board", first, &hash)
            .unwrap();
        let later = revised(&ana, &head, &bundle("board", &["card"], "two"));
        offered(&fixture.arrive(&later, false));

        let told = fixture.arrive(&withdrawn(&ana, &later), false);
        let [
            Told::Withdrawn {
                name, by, signer, ..
            },
        ] = told.as_slice()
        else {
            panic!("{told:?}")
        };
        assert_eq!(name, "board");
        assert!(by.as_ref().is_some_and(|by| by.is(signer)), "by its signer");
        assert!(fixture.waiting().is_empty());
        let runnable = fixture.apps().runnable(community, "board").unwrap();
        assert_eq!(runnable.version(), first, "the installed copy stays");

        // Carol deletes Dan's offer of an app not installed here: it is
        // told as hers, and the app, never approved, is not kept.
        let dan = Keys::generate();
        let poll = created(&dan, channel, &bundle("poll", &["vote"], "poll"));
        offered(&fixture.arrive(&poll, false));
        let told = fixture.arrive(&withdrawn(&carol, &poll), false);
        let [
            Told::Withdrawn {
                by: Some(by),
                signer,
                ..
            },
        ] = told.as_slice()
        else {
            panic!("{told:?}")
        };
        assert_eq!(by.id, Some(carol.public_key().to_hex()));
        assert_eq!(signer.id, Some(dan.public_key().to_hex()));
        assert!(matches!(
            fixture.apps().get(community, "poll"),
            Err(AppError::NotInstalled(_))
        ));

        // A set that no longer holds an offer (a sync after bzz was off).
        let rota = created(&dan, channel, &bundle("rota", &["day"], "rota"));
        offered(&fixture.arrive(&rota, false));
        let told = fixture
            .apps()
            .offers_in_set(community, channel, &HashSet::new())
            .unwrap();
        assert!(
            matches!(told.as_slice(), [Told::Withdrawn { by: None, .. }]),
            "{told:?}"
        );
        assert!(fixture.waiting().is_empty());
    }

    /// What is refused is said, and none of it is offered: a tampered
    /// artifact, an oversized one, an api bzz does not know, a name no app
    /// may have, a script that does not load (recorded as failed), an offer
    /// in a direct message.
    #[test]
    fn what_is_refused_is_said_and_never_offered() {
        use serde_json::json;
        let mut fixture = Fixture::new();
        let (community, channel) = (fixture.community, fixture.channel);
        let ana = Keys::generate();
        let raw = |content: String, app: &str| {
            let tags = [
                ["ar", "1"],
                ["d", &Uuid::new_v4().to_string()],
                ["h", &channel.to_string()],
                ["type", share::OFFER_TYPE],
                ["title", "Board"],
                ["op", "create"],
                [share::APP_TAG, app],
            ]
            .map(|parts| nostr::Tag::parse(parts).unwrap());
            parse(
                &nostr::EventBuilder::new(nostr::Kind::Custom(45_010), content)
                    .tags(tags)
                    .sign_with_keys(&ana)
                    .unwrap(),
            )
            .unwrap()
        };
        let good = script(&["card"], "one");
        let wire = |script: &str, api: i64, name: &str, hash: &str| {
            json!({"v":1,"manifest":{"name":name,"title":"Board","api":api,"types":["card"],
                   "script_sha256":hash},"brief":"A board.","script":script})
            .to_string()
        };
        let hash = crate::apps::store::manifest::sha256(good.as_bytes());
        let cases = [
            (
                raw(wire("os.exit()", 1, "board", &hash), "board"),
                "changed on the way",
            ),
            (
                raw(
                    wire(
                        &format!("--{}\n", "x".repeat(130 * 1024)),
                        1,
                        "board",
                        &hash,
                    ),
                    "board",
                ),
                "too large",
            ),
            (raw(wire(&good, 2, "board", &hash), "board"), "api = 2"),
            (
                raw(wire(&good, 1, "Board", &hash), "Board"),
                "not a name an app may have",
            ),
            (raw("{\"v\":1}".into(), "board"), "not an app offer"),
        ];
        for (head, why) in cases {
            let told = fixture.arrive(&head, false);
            let [
                Told::Refused {
                    why: said, from, ..
                },
            ] = told.as_slice()
            else {
                panic!("{why}: {told:?}")
            };
            assert!(said.contains(why), "{why}: {said}");
            assert_eq!(from.id, Some(ana.public_key().to_hex()));
        }
        assert!(fixture.apps().list(community).unwrap().is_empty());

        // A script that does not load: recorded as failed, never offered.
        let mut broken = bundle("board", &["card"], "one");
        broken.script = b"return app(".to_vec();
        let told = fixture.arrive(&created(&ana, channel, &broken), false);
        assert!(matches!(told.as_slice(), [Told::Failed { .. }]), "{told:?}");
        assert!(fixture.waiting().is_empty());
        let versions = fixture.apps().versions(community, "board").unwrap();
        assert_eq!(versions[0].state, VersionState::Failed);

        // In a direct message, no app is offered.
        let told = fixture.arrive(
            &created(&ana, channel, &bundle("poll", &["vote"], "p")),
            true,
        );
        let [Told::Refused { why, .. }] = told.as_slice() else {
            panic!("{told:?}")
        };
        assert!(why.contains("direct message"), "{why}");
        assert!(fixture.waiting().is_empty());

        // This bzz's own offer is offered to nobody here.
        let me = fixture.me.clone();
        assert!(
            fixture
                .arrive(
                    &created(&me, channel, &bundle("rota", &["day"], "r")),
                    false
                )
                .is_empty()
        );
        assert!(fixture.waiting().is_empty());
    }

    /// Not buried in offers: a bound for each person and each channel; and
    /// an offer the owner refused does not come back as the same bytes.
    #[test]
    fn offers_are_bounded_and_refused_bytes_do_not_come_back() {
        let mut fixture = Fixture::new();
        let (community, channel) = (fixture.community, fixture.channel);
        let ana = Keys::generate();
        for name in ["a", "b", "c"] {
            offered(&fixture.arrive(&created(&ana, channel, &bundle(name, &["x"], name)), false));
        }
        let told = fixture.arrive(&created(&ana, channel, &bundle("d", &["x"], "d")), false);
        let [Told::Refused { why, .. }] = told.as_slice() else {
            panic!("{told:?}")
        };
        assert!(why.contains("other offers of this person wait"), "{why}");
        // Others may still offer, up to the channel's bound.
        for index in 0..(OFFERS_PER_CHANNEL - OFFERS_PER_PERSON) {
            let name = format!("e{index}");
            offered(&fixture.arrive(
                &created(&Keys::generate(), channel, &bundle(&name, &["x"], &name)),
                false,
            ));
        }
        let told = fixture.arrive(
            &created(&Keys::generate(), channel, &bundle("z", &["x"], "z")),
            false,
        );
        let [Told::Refused { why, .. }] = told.as_slice() else {
            panic!("{told:?}")
        };
        assert!(
            why.contains("offers wait for your answer in this channel"),
            "{why}"
        );

        // The owner refuses Ana's `a`. The same bytes, offered again in a
        // new artifact, do not come back.
        let version = fixture.waiting()[0].1;
        fixture
            .apps()
            .refuse(community, "a", version, Some("no"))
            .unwrap();
        let told = fixture.arrive(&created(&ana, channel, &bundle("a", &["x"], "a")), false);
        assert!(
            matches!(told.as_slice(), [Told::Declined { .. }]),
            "{told:?}"
        );
        // Other bytes of the same app are a new offer.
        offered(&fixture.arrive(&created(&ana, channel, &bundle("a", &["x"], "a2")), false));
    }
}
