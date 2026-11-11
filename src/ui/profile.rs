//! Profile card: who a person is in this community. Names, the relay-verified
//! NIP-FI identity (never raw claims), status, presence and — where the relay
//! offers NIP-CL `membership_public` — their listed, open channels.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use uuid::Uuid;

use crate::{
    domain::{MemberRole, Profile},
    protocol::membership_public::PublicMembership,
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

/// State of the public-channel lookup for the open card.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicChannels {
    Loading,
    /// The identity is locked: no NIP-98 request can be signed.
    Locked,
    /// The relay does not offer the lookup (or it is backing off).
    Unsupported,
    RateLimited(u64),
    /// The lookup failed; never shown as an empty list.
    Failed,
    Verified(Vec<PublicMembership>),
}

#[derive(Clone, Debug)]
pub struct ProfileCard {
    pub pubkey: String,
    pub profile: Option<Profile>,
    pub public: PublicChannels,
    /// A previously verified list shown, marked stale, while a refresh is
    /// loading or after it failed.
    pub stale: Option<Vec<PublicMembership>>,
    /// The one outstanding lookup. An answer is applied only when it carries
    /// this generation, then the slot empties: consumed at most once.
    pub outstanding: Option<u64>,
}

impl ProfileCard {
    pub const fn new(pubkey: String, profile: Option<Profile>) -> Self {
        Self {
            pubkey,
            profile,
            public: PublicChannels::Loading,
            stale: None,
            outstanding: None,
        }
    }
}

const fn role_label(role: MemberRole) -> &'static str {
    match role {
        MemberRole::Owner => "owner",
        MemberRole::Admin => "admin",
        MemberRole::Member => "member",
        MemberRole::Bot => "bot",
        MemberRole::Guest => "guest",
        MemberRole::Unknown => "unknown",
    }
}

fn channel_lines(
    memberships: &[PublicMembership],
    channel_name: &dyn Fn(Uuid) -> Option<String>,
) -> Vec<Line<'static>> {
    if memberships.is_empty() {
        return vec![Line::from(
            "  none (no listed, open channel at this moment)",
        )];
    }
    memberships
        .iter()
        .take(40)
        .map(|membership| {
            let name = channel_name(membership.channel_id).map_or_else(
                || format!("(not cached) {}", &membership.channel_id.to_string()[..8]),
                |name| format!("#{}", sanitize::single_line(&name)),
            );
            Line::from(format!(
                "  {name} · {} · {}{}",
                membership.channel_type,
                role_label(membership.role),
                if membership.archived {
                    " · archived"
                } else {
                    ""
                }
            ))
        })
        .chain(
            (memberships.len() > 40)
                .then(|| Line::from(format!("  … and {} more", memberships.len() - 40))),
        )
        .collect()
}

/// The card's text, separate from drawing so tests can assert it.
pub fn lines(
    card: &ProfileCard,
    now: u64,
    channel_name: &dyn Fn(Uuid) -> Option<String>,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let muted = theme.style(HighlightGroup::Muted);
    let mut lines = Vec::new();
    let label = card.profile.as_ref().map_or_else(
        || crate::domain::abbreviated_pubkey(&card.pubkey),
        Profile::author_label,
    );
    let presence = card.profile.as_ref().map_or("", Profile::presence_marker);
    lines.push(Line::from(format!(
        "{presence}{}",
        sanitize::single_line(&label)
    )));
    if let Some(profile) = &card.profile {
        if let Some(verified) = &profile.verified {
            let mut line = format!(
                "✓ verified by {} · {}",
                sanitize::single_line(&verified.issuer_label),
                sanitize::single_line(&verified.username)
            );
            if let Some(full_name) = &verified.full_name {
                line.push_str(&format!(" · {}", sanitize::single_line(full_name)));
            }
            lines.push(Line::from(line));
        }
        if let Some(name) = profile.name.as_deref().filter(|name| !name.is_empty()) {
            lines.push(Line::from(format!("name: {}", sanitize::single_line(name))));
        }
        if let Some(nip05) = profile.nip05.as_deref().filter(|value| !value.is_empty()) {
            lines.push(Line::from(vec![
                Span::raw(format!("nip-05: {} ", sanitize::single_line(nip05))),
                Span::styled("(not checked)", muted),
            ]));
        }
        if let Some(status) = profile.status_text(now) {
            lines.push(Line::from(format!(
                "status: {}",
                sanitize::single_line(&status)
            )));
        }
        if let Some(about) = profile.about.as_deref().filter(|about| !about.is_empty()) {
            let about = sanitize::single_line(about);
            let about = about.chars().take(280).collect::<String>();
            lines.push(Line::from(format!("about: {about}")));
        }
    }
    let npub = nostr::PublicKey::from_hex(&card.pubkey)
        .ok()
        .and_then(|key| nostr::nips::nip19::ToBech32::to_bech32(&key).ok())
        .unwrap_or_else(|| card.pubkey.clone());
    lines.push(Line::from(Span::styled(npub, muted)));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "public channels (listed and open; private, unlisted and DMs never appear)",
        muted,
    )));
    let stale = |lines: &mut Vec<Line<'static>>| {
        if let Some(stale) = &card.stale {
            lines.push(Line::from(Span::styled(
                "  stale — last verified list:",
                muted,
            )));
            lines.extend(channel_lines(stale, channel_name));
        }
    };
    match &card.public {
        PublicChannels::Loading => {
            lines.push(Line::from("  loading…"));
            stale(&mut lines);
        }
        PublicChannels::Locked => {
            lines.push(Line::from(
                "  unlock the identity to look up public channels",
            ));
            stale(&mut lines);
        }
        PublicChannels::Unsupported => {
            lines.push(Line::from(
                "  this relay does not offer public membership lookup",
            ));
            stale(&mut lines);
        }
        PublicChannels::RateLimited(seconds) => {
            lines.push(Line::from(format!(
                "  rate limited by the relay; retry in {seconds}s"
            )));
            stale(&mut lines);
        }
        PublicChannels::Failed => {
            lines.push(Line::from("  could not verify the relay's answer"));
            stale(&mut lines);
        }
        PublicChannels::Verified(memberships) => {
            lines.extend(channel_lines(memberships, channel_name));
        }
    }
    lines
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    card: &ProfileCard,
    now: u64,
    channel_name: &dyn Fn(Uuid) -> Option<String>,
    theme: &Theme,
) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Picker))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(" profile · r refresh · Esc close ");
    frame.render_widget(
        Paragraph::new(lines(card, now, channel_name, theme))
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: true })
            .block(block),
        area,
    );
}
