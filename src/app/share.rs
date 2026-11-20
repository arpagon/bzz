//! Sharing an app in the channel, inside bzz (v0.16 M6).
//!
//! ```text
//!   :app share [name]      the app open in this channel (or `name`): its
//!                          version that runs, offered here, signed by you;
//!                          its brief posted here once for that version
//!   :app unshare [name]    the offer withdrawn: a delete revision
//! ```
//!
//! **Offers arrive** on the offers' own sync of the channel shown
//! (`service::offers`), which this loop points at the selected channel on
//! every tick. An offer becomes a version that waits, announced by the
//! banner and answered in M4's overlay (`Ctrl+Y`), as any other proposal:
//! what arrives is told on the status line, and nothing of it runs before
//! the owner approves its exact bytes.

use super::App;
use crate::error::Result;

use crate::{
    apps::store::{Proposer, offers::Told},
    render::sanitize::single_line,
    service::offers::{OfferNews, OffersService},
    ui::state::Route,
};

/// Lines of what happened to offers, kept for `:apps`.
pub const OFFER_NOTES: usize = 6;

fn who(proposer: &Proposer) -> String {
    single_line(&crate::protocol::artifact::display(
        proposer.name.as_deref().unwrap_or("someone"),
        32,
    ))
}

/// What the owner is told of one thing that came of an offer.
pub fn told_text(told: &Told) -> String {
    match told {
        Told::Offered {
            name,
            version,
            from,
            replaces,
            types_changed,
            ..
        } => {
            let types = if *types_changed {
                ", with other record types than the version that runs"
            } else {
                ""
            };
            match replaces {
                Some(was) => format!(
                    "{} published a new revision of {}'s offer of {name}: it is {}'s offer now (version {version}{types}) · Ctrl-Y to read it",
                    who(from),
                    who(was),
                    who(from)
                ),
                None => format!(
                    "{} offers {name} (version {version}{types}) in this channel · Ctrl-Y to read it; nothing of it runs before you approve it",
                    who(from)
                ),
            }
        }
        Told::Installed { name, from } => format!(
            "{} offers {name}, the version that runs here: nothing to approve",
            who(from)
        ),
        Told::Declined { name, from } => format!(
            "{} offered {name} again as the bytes you refused: not shown again",
            who(from)
        ),
        Told::Failed { name, from, error } => format!(
            "{} offered {name}, and it does not load, so it is not offered: {}",
            who(from),
            single_line(error.lines().next().unwrap_or_default())
        ),
        Told::Refused { name, from, why } => format!(
            "an offer of {} from {} was refused: {}",
            name.as_deref().unwrap_or("an app"),
            who(from),
            single_line(why)
        ),
        Told::Withdrawn {
            name, signer, by, ..
        } => match by {
            Some(by) if by.is(signer) => format!(
                "{} withdrew the offer of {name}; it no longer waits, and a copy you installed stays",
                who(signer)
            ),
            Some(by) => format!(
                "{} deleted {}'s offer of {name}; it no longer waits, and a copy you installed stays",
                who(by),
                who(signer)
            ),
            None => format!(
                "the offer of {name} from {} is no longer in the channel; it no longer waits",
                who(signer)
            ),
        },
    }
}

impl App {
    /// The offers service of the community bzz is connected to, made when
    /// it is first needed. None while bzz is not connected.
    fn offers(&mut self) -> Option<OffersService> {
        let runtime = self.runtime.as_ref()?;
        if let Some((community, offers)) = &self.apps.offers
            && *community == runtime.community_id
        {
            return Some(offers.clone());
        }
        if let Some((_, old)) = self.apps.offers.take() {
            old.watch(None);
        }
        let sender = self.background_tx.clone();
        let offers = OffersService::new(
            runtime.community_id,
            runtime.http.clone(),
            self.store.clone(),
            runtime.signer.clone(),
            runtime.supervisor.clone(),
            runtime.messages.clone(),
            self.paths.apps_dir(),
            std::sync::Arc::new(move |news| {
                // Never waits: a full channel loses a line of the status
                // bar, and the store, asked every second, has the offer.
                let _ = sender.try_send(super::Background::AppOffers(news));
            }),
        );
        self.apps.offers = Some((runtime.community_id, offers.clone()));
        Some(offers)
    }

    /// Every tick: the offers' sync follows the channel shown.
    pub(super) fn watch_offers(&mut self) {
        let wanted = if self.presentation.route == Route::Workspace {
            self.current_channel()
                .map(|channel| (channel.id, channel.kind.is_dm()))
        } else {
            None
        };
        if self.runtime.is_none() {
            self.stop_offers();
            return;
        }
        if let Some(offers) = self.offers() {
            offers.watch(wanted);
        }
    }

    /// Ends the offers' sync: bzz locks, quits, or leaves the community.
    pub(super) fn stop_offers(&mut self) {
        if let Some((_, offers)) = self.apps.offers.take() {
            offers.watch(None);
        }
    }

    /// What the offers' sync found.
    pub(super) fn offer_news(&mut self, news: OfferNews) {
        let lines: Vec<String> = match news {
            OfferNews::Told { told, .. } => told.iter().map(told_text).collect(),
            OfferNews::Synced { .. } => Vec::new(),
            OfferNews::Stopped { why, .. } => vec![format!("offers of apps: {why}")],
        };
        if lines.is_empty() {
            return;
        }
        for line in &lines {
            if self.apps.offer_notes.len() >= OFFER_NOTES {
                self.apps.offer_notes.remove(0);
            }
            self.apps.offer_notes.push(line.clone());
        }
        // The banner and Ctrl-Y read what waits from the store.
        self.apps.checked = None;
        self.status_error = lines.last().cloned();
    }

    /// `:app share [name]` and `:app unshare [name]`.
    pub(super) async fn share_command(&mut self, share: bool, name: Option<&str>) -> Result<()> {
        let Some(channel) = self.current_channel().cloned() else {
            self.status_error = Some("select a channel first".into());
            return Ok(());
        };
        if channel.kind.is_dm() {
            self.status_error =
                Some("an app cannot be shared in a direct message; share it in a channel".into());
            return Ok(());
        }
        let shown = self
            .shown_app()
            .map(|(_, name)| name.to_owned())
            .filter(|_| !self.dev_open_in(channel.id));
        let name = match (name, shown) {
            (Some(name), _) => name.to_owned(),
            (None, Some(name)) => name,
            (None, None) => {
                self.status_error = Some(if share {
                    "no approved app is open in this channel: :app open <name>, then :app share; or :app share <name>"
                        .into()
                } else {
                    "usage: :app unshare <name>".into()
                });
                return Ok(());
            }
        };
        let Some(offers) = self.offers() else {
            self.status_error = Some(
                "sharing an app needs the relay: unlock the identity and connect first".into(),
            );
            return Ok(());
        };
        let place = format!("#{}", single_line(&channel.name));
        let tx = self.background_tx.clone();
        self.status_error = Some(if share {
            format!("sharing {name} in {place}…")
        } else {
            format!("withdrawing the offer of {name} from {place}…")
        });
        tokio::spawn(async move {
            let said = if share {
                match offers.share(channel.id, &name).await {
                    Ok(shared) if shared.unchanged => format!(
                        "{name}, version {}, is shared in {place} already{}",
                        shared.version,
                        if shared.brief_posted {
                            "; its brief was posted there"
                        } else {
                            ""
                        }
                    ),
                    Ok(shared) => format!(
                        "{name}, version {}, is shared in {place}{}{} · :app unshare withdraws it",
                        shared.version,
                        if shared.revised {
                            " in place of the version shared before"
                        } else {
                            ""
                        },
                        if shared.brief_posted {
                            "; its brief was posted there"
                        } else {
                            ""
                        }
                    ),
                    Err(why) => format!("{name} was not shared: {why}"),
                }
            } else {
                match offers.unshare(channel.id, &name).await {
                    Ok(unshared) => match unshared.already_by {
                        Some(by) => format!(
                            "the offer of {name} in {place} was withdrawn already, by {}",
                            crate::domain::abbreviated_pubkey(&by)
                        ),
                        None => format!(
                            "the offer of {name} is withdrawn from {place}; copies people installed stay"
                        ),
                    },
                    Err(why) => format!("the offer of {name} was not withdrawn: {why}"),
                }
            };
            let _ = tx.send(super::Background::Notice(said)).await;
        });
        Ok(())
    }
}
