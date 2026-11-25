//! The join form (v0.17 A9): from a community's address to its first
//! channel without leaving bzz. Its steps are the CLI's (`bzz::join`): the
//! address is read and checked, the person says who they are there, and the
//! relay is asked whether that identity may come in.
//!
//! This module is the form: its state, its keys and its screen. What it
//! asks of the network and of the configuration is done by `app::join`
//! through `bzz::join`, `bzz::auth` and `bzz::auth::pairing`.
//!
//! Secrets typed here (a passphrase, an nsec) are held in `Zeroizing`
//! buffers, drawn as dots, never in a `Debug`, a notice or a diagnostic,
//! and wiped when the form moves on, as `bzz identity new` and `import`
//! hold what they read from the terminal.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use uuid::Uuid;
use zeroize::{Zeroize as _, Zeroizing};

use crate::{
    auth::federated::Requirement,
    config::{IdentityConfig, KeyBackend},
    join::{Address, RelayFacts},
    render::sanitize,
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

/// Where the form is opened from. The first screen's three choices, and
/// the `:` line with or without an address (`:join <address>`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum JoinStart {
    /// "Join a community": a new identity is the usual answer.
    #[default]
    Join,
    /// "Pair with Buzz": pairing with Buzz desktop is chosen first.
    Pair,
    /// "Import a key": importing is chosen first.
    Import,
    /// `:join <address>`: the address is filled in and checked at once.
    Address(String),
}

/// How long typing pauses before the address is checked.
pub const CHECK_AFTER: Duration = Duration::from_millis(400);
/// The calm pace of asking again while an admin has not let you in yet:
/// 15 s, 30 s, then every minute.
pub fn retry_after(attempts: u32) -> Duration {
    Duration::from_secs(match attempts {
        0 | 1 => 15,
        2 => 30,
        _ => 60,
    })
}
/// The shortest passphrase the encrypted file takes (`auth::encrypted_file`).
pub const MIN_PASSPHRASE: usize = 12;

/// What the address field is known to be.
#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    /// Nothing typed, or typing.
    Idle,
    /// Asked; the answer has not come.
    Checking(String),
    /// Something answers there, and this is what it says.
    Known {
        input: String,
        address: Address,
        facts: RelayFacts,
    },
    /// The address cannot be read, or nothing usable answers there: what
    /// happened and what to try.
    Bad { what: String, next: String },
}

/// Who the person is in the community: the choices, the usual one first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Who {
    /// A new identity made here (the newcomer's answer).
    New,
    /// An identity bzz already holds.
    Held(Uuid),
    /// Pair with Buzz desktop on this machine.
    Pair,
    /// Sign in through the browser (NIP-FA), where the community offers it.
    Browser,
    /// Import a key bzz is given in a hidden field.
    Import,
}

/// One row of the choice of identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Choice {
    pub who: Who,
    pub label: String,
    pub hint: String,
    /// Why it cannot be chosen here, when it cannot.
    pub unavailable: Option<String>,
    /// A held identity in an encrypted file: its passphrase is asked.
    pub unlock: bool,
}

/// Which hidden or plain field is being typed after the choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    /// The nsec or hex secret of "import a key".
    Secret,
    /// A new passphrase for the encrypted file, then its confirmation.
    NewPassphrase,
    ConfirmPassphrase,
    /// The passphrase of a held identity kept in an encrypted file.
    Unlock,
    /// The address of the community's sign-in service (NIP-FA adapter).
    Adapter,
}

/// The step the form is at.
#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    /// The one field: the community's address or an invite.
    Address,
    /// Who you are there.
    Who,
    /// A field after the choice: a secret, a passphrase, an address.
    Typing(Field),
    /// bzz is at work; the line says at what.
    Working(String),
    /// The community lets in its members only, and you are not one yet.
    Waiting,
    /// It did not work: what happened and what to try.
    Failed { what: String, next: String },
    /// Leaving a join whose identity was made here: keep it or not.
    Keep,
    /// `bzz demo`: the form explains that it needs a real address.
    Demo,
}

/// What the form asks of bzz after a key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Step {
    Nothing,
    /// Leave the screen (a join that waits keeps waiting).
    Close,
    /// Check the address now.
    Check,
    /// Do what the form now holds: make or unlock the identity, claim the
    /// invite, ask the relay.
    Start,
    /// Copy this text (the npub).
    Copy(String),
    /// Ask the relay again now.
    RetryNow,
    /// Stop this join. `keep`: the identity made for it stays; `leave`:
    /// the screen closes too, else it is back at the address.
    Abandon {
        keep: bool,
        leave: bool,
    },
}

/// The join form.
pub struct JoinForm {
    pub stage: Stage,
    pub address: String,
    pub check: Check,
    /// When the address was last edited, for the pause before checking.
    pub edited_at: Option<Instant>,
    /// Enter was pressed while the check was still out.
    pub continue_when_known: bool,
    pub choices: Vec<Choice>,
    pub selected: usize,
    /// This system has a keychain (`auth::keychain::available`).
    pub keychain: bool,
    secret: Zeroizing<String>,
    passphrase: Zeroizing<String>,
    confirm: Zeroizing<String>,
    pub adapter: String,
    /// What was chosen, once it was.
    pub chosen: Option<Who>,
    /// The identity this join uses, once it exists; `made_here` when this
    /// form made it (and may take it back).
    pub identity: Option<IdentityConfig>,
    pub made_here: bool,
    /// The person said to keep the identity made here whatever happens.
    pub keep: bool,
    /// The identity's npub, to give to an admin.
    pub npub: Option<String>,
    /// Asking the relay again: when next, how many times so far, and
    /// whether an answer is out now.
    pub next_try: Option<Instant>,
    pub attempts: u32,
    pub asking: bool,
    /// A short line under the steps: copied, asked again, why not.
    pub notice: Option<String>,
}

impl std::fmt::Debug for JoinForm {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the secret fields, whatever they hold.
        formatter
            .debug_struct("JoinForm")
            .field("stage", &self.stage)
            .field("address", &self.address)
            .field("check", &self.check)
            .field("chosen", &self.chosen)
            .field(
                "identity",
                &self.identity.as_ref().map(|identity| identity.id),
            )
            .finish_non_exhaustive()
    }
}

impl JoinForm {
    pub fn new(start: &JoinStart, keychain: bool) -> Self {
        let mut form = Self {
            stage: Stage::Address,
            address: String::new(),
            check: Check::Idle,
            edited_at: None,
            continue_when_known: false,
            choices: Vec::new(),
            selected: 0,
            keychain,
            secret: Zeroizing::new(String::new()),
            passphrase: Zeroizing::new(String::new()),
            confirm: Zeroizing::new(String::new()),
            adapter: String::new(),
            chosen: match start {
                JoinStart::Pair => Some(Who::Pair),
                JoinStart::Import => Some(Who::Import),
                JoinStart::Join | JoinStart::Address(_) => None,
            },
            identity: None,
            made_here: false,
            keep: false,
            npub: None,
            next_try: None,
            attempts: 0,
            asking: false,
            notice: None,
        };
        if let JoinStart::Address(address) = start {
            form.address = sanitize::single_line(address).trim().to_owned();
        }
        form
    }

    /// The form under `bzz demo`: it says what joining needs.
    pub fn demo() -> Self {
        let mut form = Self::new(&JoinStart::Join, false);
        form.stage = Stage::Demo;
        form
    }

    /// The address and what answers there, once known.
    pub fn known(&self) -> Option<(&Address, &RelayFacts)> {
        match &self.check {
            Check::Known {
                input,
                address,
                facts,
            } if *input == self.address.trim() => Some((address, facts)),
            _ => None,
        }
    }

    /// The address was edited and has rested long enough to be checked.
    pub fn check_due(&self, now: Instant) -> bool {
        self.stage == Stage::Address
            && self
                .edited_at
                .is_some_and(|edited| now.saturating_duration_since(edited) >= CHECK_AFTER)
    }

    pub fn secret(&self) -> &Zeroizing<String> {
        &self.secret
    }

    pub fn passphrase(&self) -> &Zeroizing<String> {
        &self.passphrase
    }

    /// Wipes every secret field.
    pub fn wipe(&mut self) {
        self.secret.zeroize();
        self.passphrase.zeroize();
        self.confirm.zeroize();
    }

    /// The choices once the address is known: the usual one first, and the
    /// one the first screen asked for selected.
    pub fn set_choices(&mut self, choices: Vec<Choice>) {
        let wanted = self.chosen.clone();
        self.selected = wanted
            .and_then(|wanted| choices.iter().position(|choice| choice.who == wanted))
            .or_else(|| {
                choices
                    .iter()
                    .position(|choice| choice.unavailable.is_none())
            })
            .unwrap_or(0);
        self.choices = choices;
    }

    /// A paste: into the field being typed, as one piece of text.
    pub fn paste(&mut self, text: &str) {
        let text = Zeroizing::new(text.trim().to_owned());
        match self.stage {
            Stage::Address => {
                self.address.push_str(&sanitize::single_line(&text));
                self.edited_at = Some(Instant::now());
            }
            Stage::Typing(field) => {
                if let Some(buffer) = self.field_mut(field) {
                    buffer.push_str(&text);
                }
            }
            _ => {}
        }
    }

    fn field_mut(&mut self, field: Field) -> Option<&mut String> {
        match field {
            Field::Secret => Some(&mut self.secret),
            Field::NewPassphrase | Field::Unlock => Some(&mut self.passphrase),
            Field::ConfirmPassphrase => Some(&mut self.confirm),
            Field::Adapter => Some(&mut self.adapter),
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> Step {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match self.stage.clone() {
            Stage::Demo => match key.code {
                KeyCode::Esc | KeyCode::Enter => Step::Close,
                _ => Step::Nothing,
            },
            Stage::Address => match key.code {
                KeyCode::Esc => Step::Close,
                KeyCode::Enter => self.enter_address(),
                KeyCode::Backspace => {
                    self.address.pop();
                    self.edited(true)
                }
                KeyCode::Char('u') if control => {
                    self.address.clear();
                    self.edited(true)
                }
                KeyCode::Char(character) if !control && !character.is_control() => {
                    if self.address.chars().count() < 2048 {
                        self.address.push(character);
                    }
                    self.edited(true)
                }
                _ => Step::Nothing,
            },
            Stage::Who => match key.code {
                KeyCode::Esc => {
                    self.stage = Stage::Address;
                    Step::Nothing
                }
                KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
                    self.move_choice(1);
                    Step::Nothing
                }
                KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
                    self.move_choice(-1);
                    Step::Nothing
                }
                KeyCode::Enter => self.choose(),
                _ => Step::Nothing,
            },
            Stage::Typing(field) => self.typing_key(field, key, control),
            Stage::Working(_) => match key.code {
                KeyCode::Esc => Step::Close,
                _ => Step::Nothing,
            },
            Stage::Waiting => match key.code {
                KeyCode::Esc => Step::Close,
                KeyCode::Char('c') => self.npub.clone().map_or(Step::Nothing, Step::Copy),
                KeyCode::Char('r') | KeyCode::Enter if !self.asking => Step::RetryNow,
                KeyCode::Char('x') if self.made_here => {
                    self.stage = Stage::Keep;
                    Step::Nothing
                }
                KeyCode::Char('x') => Step::Abandon {
                    keep: true,
                    leave: true,
                },
                _ => Step::Nothing,
            },
            Stage::Failed { .. } => match key.code {
                KeyCode::Char('k') if self.made_here && !self.keep => {
                    self.keep = true;
                    self.notice = Some("the identity made for this join stays".into());
                    Step::Nothing
                }
                KeyCode::Enter | KeyCode::Esc => Step::Abandon {
                    keep: self.keep || !self.made_here,
                    leave: key.code == KeyCode::Esc,
                },
                _ => Step::Nothing,
            },
            Stage::Keep => match key.code {
                KeyCode::Char('y' | 'Y') => Step::Abandon {
                    keep: true,
                    leave: true,
                },
                KeyCode::Char('n' | 'N') => Step::Abandon {
                    keep: false,
                    leave: true,
                },
                KeyCode::Esc => {
                    self.stage = Stage::Waiting;
                    Step::Nothing
                }
                _ => Step::Nothing,
            },
        }
    }

    /// Back to the address, with what this join had made forgotten by the
    /// form (`app::join` takes it back from the configuration).
    pub fn back_to_address(&mut self) {
        self.wipe();
        self.stage = Stage::Address;
        self.identity = None;
        self.made_here = false;
        self.keep = false;
        self.npub = None;
        self.next_try = None;
        self.attempts = 0;
        self.asking = false;
    }

    fn edited(&mut self, changed: bool) -> Step {
        if changed {
            self.edited_at = Some(Instant::now());
            self.continue_when_known = false;
            self.notice = None;
            if self.address.trim().is_empty() {
                self.check = Check::Idle;
                self.edited_at = None;
            }
        }
        Step::Nothing
    }

    fn enter_address(&mut self) -> Step {
        if self.known().is_some() {
            self.stage = Stage::Who;
            return Step::Nothing;
        }
        self.continue_when_known = true;
        match &self.check {
            Check::Checking(input) if *input == self.address.trim() => Step::Nothing,
            _ => Step::Check,
        }
    }

    fn move_choice(&mut self, delta: isize) {
        if self.choices.is_empty() {
            return;
        }
        let count = self.choices.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
        self.notice = None;
    }

    fn choose(&mut self) -> Step {
        let Some(choice) = self.choices.get(self.selected).cloned() else {
            return Step::Nothing;
        };
        if let Some(reason) = choice.unavailable {
            self.notice = Some(reason);
            return Step::Nothing;
        }
        self.chosen = Some(choice.who.clone());
        self.notice = None;
        self.wipe();
        match choice.who {
            Who::New if !self.keychain => self.stage = Stage::Typing(Field::NewPassphrase),
            Who::Import => self.stage = Stage::Typing(Field::Secret),
            Who::Browser => self.stage = Stage::Typing(Field::Adapter),
            Who::Held(_) if choice.unlock => {
                self.stage = Stage::Typing(Field::Unlock);
            }
            _ => return Step::Start,
        }
        Step::Nothing
    }

    fn typing_key(&mut self, field: Field, key: KeyEvent, control: bool) -> Step {
        match key.code {
            KeyCode::Esc => {
                self.wipe();
                self.notice = None;
                self.stage = Stage::Who;
                Step::Nothing
            }
            KeyCode::Backspace => {
                if let Some(buffer) = self.field_mut(field) {
                    buffer.pop();
                }
                Step::Nothing
            }
            KeyCode::Char('u') if control => {
                if let Some(buffer) = self.field_mut(field) {
                    buffer.zeroize();
                }
                Step::Nothing
            }
            KeyCode::Tab if field == Field::ConfirmPassphrase => {
                self.stage = Stage::Typing(Field::NewPassphrase);
                Step::Nothing
            }
            KeyCode::Tab if field == Field::NewPassphrase => {
                self.stage = Stage::Typing(Field::ConfirmPassphrase);
                Step::Nothing
            }
            KeyCode::Enter => self.field_done(field),
            KeyCode::Char(character) if !control && !character.is_control() => {
                if let Some(buffer) = self.field_mut(field)
                    && buffer.chars().count() < 1024
                {
                    buffer.push(character);
                }
                Step::Nothing
            }
            _ => Step::Nothing,
        }
    }

    fn field_done(&mut self, field: Field) -> Step {
        self.notice = None;
        match field {
            Field::Secret => {
                let parsed = nostr::Keys::parse(self.secret.trim()).is_ok();
                if !parsed {
                    self.notice =
                        Some("that is not a Nostr secret: an nsec1… or 64 hex characters".into());
                    return Step::Nothing;
                }
                if self.keychain {
                    Step::Start
                } else {
                    self.stage = Stage::Typing(Field::NewPassphrase);
                    Step::Nothing
                }
            }
            Field::NewPassphrase => {
                if self.passphrase.chars().count() < MIN_PASSPHRASE {
                    self.notice = Some(format!(
                        "a passphrase of {MIN_PASSPHRASE} characters or more"
                    ));
                    return Step::Nothing;
                }
                self.stage = Stage::Typing(Field::ConfirmPassphrase);
                Step::Nothing
            }
            Field::ConfirmPassphrase => {
                if self.passphrase.as_str() != self.confirm.as_str() {
                    self.confirm.zeroize();
                    self.notice = Some("the two passphrases differ; type the second again".into());
                    return Step::Nothing;
                }
                Step::Start
            }
            Field::Unlock => {
                if self.passphrase.is_empty() {
                    return Step::Nothing;
                }
                Step::Start
            }
            Field::Adapter => match crate::auth::federated::AdapterClient::new(
                match url::Url::parse(self.adapter.trim()) {
                    Ok(url) => url,
                    Err(_) => {
                        self.notice = Some("the sign-in service's address: https://…".into());
                        return Step::Nothing;
                    }
                },
            ) {
                Ok(_) if self.keychain => Step::Start,
                Ok(_) => {
                    self.stage = Stage::Typing(Field::NewPassphrase);
                    Step::Nothing
                }
                Err(error) => {
                    self.notice = Some(error.to_string());
                    Step::Nothing
                }
            },
        }
    }
}

/// What the community is, in one line: its name, and who it lets in.
pub fn describe(address: &Address, facts: &RelayFacts) -> String {
    let relay = address.relay();
    let name = facts.label(relay);
    let mut line = if facts.name.is_some() {
        format!("{name} · a Buzz community at {}", relay.host())
    } else {
        format!("{name}: a Buzz community")
    };
    line.push_str(match (address.invite().is_some(), facts.members_only) {
        (true, _) => " · your invite lets you in",
        (false, true) => " · it lets in its members only",
        (false, false) => " · open: anyone who signs in may join",
    });
    if facts.federated == Requirement::Required {
        line.push_str(" · sign-in through its identity provider");
    }
    if relay.insecure {
        line.push_str(" · on this machine, without TLS");
    }
    line
}

/// Where the key of a new identity is kept, in one line.
pub fn kept_where(keychain: bool) -> &'static str {
    if keychain {
        "its key kept in this system's keychain"
    } else {
        "no keychain here: key in a passphrase-locked file"
    }
}

/// The npub shown to give to an admin, shortened only when it must be.
fn npub_line(npub: &str, width: usize) -> String {
    if npub.chars().count() + 4 <= width {
        npub.to_owned()
    } else {
        crate::domain::abbreviated_pubkey(npub)
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, form: &JoinForm, theme: &Theme) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Modal))
        .border_style(theme.style(HighlightGroup::ModalBorder))
        .title_style(theme.style(HighlightGroup::ModalTitle))
        .title(" Join a community ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = usize::from(inner.width.saturating_sub(2));
    let muted = theme.style(HighlightGroup::Muted);
    let strong = theme.style(HighlightGroup::Strong);
    let pad = |text: String| format!(" {text}");
    let mut lines: Vec<Line<'static>> = vec![Line::default()];

    if form.stage == Stage::Demo {
        lines.push(Line::styled(
            pad("This is the demo: a generated team, offline.".into()),
            strong,
        ));
        lines.push(Line::default());
        for text in [
            "Joining needs a real community's address, and bzz keeps what it",
            "makes for you (an identity, the community) in your own settings.",
            "The demo writes nothing there.",
            "",
            "To join your team: leave the demo with q, start bzz, and press j",
            "(or type :join) on its first screen.",
        ] {
            lines.push(Line::from(pad(text.into())));
        }
        lines.push(Line::default());
        lines.push(Line::styled(pad("Esc back to the demo".into()), muted));
        draw(frame, inner, lines, theme);
        return;
    }

    // 1. The address.
    let address_focused = form.stage == Stage::Address;
    lines.push(Line::styled(
        pad("Community address, or the invite link your admin sent".into()),
        if address_focused { strong } else { muted },
    ));
    let shown: String = {
        let address = sanitize::single_line(&form.address);
        let room = width.saturating_sub(4);
        let count = address.chars().count();
        address.chars().skip(count.saturating_sub(room)).collect()
    };
    lines.push(Line::from(vec![
        Span::raw(if address_focused { " › " } else { "   " }),
        Span::styled(
            shown,
            theme.style(if address_focused {
                HighlightGroup::Composer
            } else {
                HighlightGroup::Normal
            }),
        ),
        Span::raw(if address_focused { "▏" } else { "" }),
    ]));
    match &form.check {
        Check::Idle if form.address.trim().is_empty() => lines.push(Line::styled(
            pad("  e.g. my-team.communities.buzz.xyz".into()),
            muted,
        )),
        Check::Idle => {}
        Check::Checking(_) => lines.push(Line::styled(pad("  checking…".into()), muted)),
        Check::Known { address, facts, .. } if form.known().is_some() => {
            lines.push(Line::styled(
                pad(format!("  ✓ {}", describe(address, facts))),
                theme.style(HighlightGroup::Success),
            ));
            if let Some(description) = &facts.description {
                lines.push(Line::styled(pad(format!("    {description}")), muted));
            }
        }
        Check::Known { .. } => {}
        Check::Bad { what, next } => {
            lines.push(Line::styled(
                pad(format!("  ✗ {what}")),
                theme.style(HighlightGroup::Error),
            ));
            lines.push(Line::styled(pad(format!("    {next}")), muted));
        }
    }
    lines.push(Line::default());

    // 2. Who you are there.
    let past_address = !matches!(form.stage, Stage::Address);
    if past_address && !form.choices.is_empty() {
        let who_focused = form.stage == Stage::Who;
        lines.push(Line::styled(
            pad("Who you are there".into()),
            if who_focused { strong } else { muted },
        ));
        let label_width = form
            .choices
            .iter()
            .map(|choice| choice.label.chars().count())
            .max()
            .unwrap_or(0)
            .min(30);
        for (index, choice) in form.choices.iter().enumerate() {
            let selected = index == form.selected;
            if !who_focused && !selected {
                continue;
            }
            let marker = if selected && who_focused {
                " › "
            } else {
                "   "
            };
            let label: String = choice.label.chars().take(label_width).collect();
            let style = match (&choice.unavailable, selected && who_focused) {
                (_, true) => theme.style(HighlightGroup::SelectedRow),
                (Some(_), false) => muted,
                (None, false) => theme.style(HighlightGroup::Normal),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{marker}{label:<label_width$}"), style),
                Span::styled(format!("  {}", choice.hint), muted),
            ]));
        }
        if let Some(Choice {
            unavailable: Some(reason),
            ..
        }) = form.choices.get(form.selected)
            && who_focused
        {
            lines.push(Line::styled(pad(format!("   {reason}")), muted));
        }
        lines.push(Line::default());
    }

    // 3. The field after the choice, the work, the wait, the failure.
    match &form.stage {
        Stage::Typing(field) => {
            let (title, value, hidden, help) = match field {
                Field::Secret => (
                    "Your key: an nsec1… or 64 hex characters",
                    form.secret.as_str(),
                    true,
                    "typed or pasted here, hidden; bzz keeps it as `bzz identity import` does",
                ),
                Field::NewPassphrase => (
                    "A passphrase for the file that keeps the key",
                    form.passphrase.as_str(),
                    true,
                    "12 characters or more; bzz asks for it each time it starts",
                ),
                Field::ConfirmPassphrase => (
                    "The same passphrase again",
                    form.confirm.as_str(),
                    true,
                    "Tab back to the first",
                ),
                Field::Unlock => (
                    "The passphrase of this identity",
                    form.passphrase.as_str(),
                    true,
                    "the one you chose when it was made",
                ),
                Field::Adapter => (
                    "The address of its sign-in service (from your admin)",
                    form.adapter.as_str(),
                    false,
                    "https://…; after joining, bzz opens your browser to sign in",
                ),
            };
            lines.push(Line::styled(pad(title.into()), strong));
            let shown = if hidden {
                "•".repeat(value.chars().count().min(width.saturating_sub(4)))
            } else {
                sanitize::single_line(value)
            };
            lines.push(Line::from(vec![
                Span::raw(" › "),
                Span::styled(shown, theme.style(HighlightGroup::Composer)),
                Span::raw("▏"),
            ]));
            lines.push(Line::styled(pad(format!("  {help}")), muted));
            lines.push(Line::default());
        }
        Stage::Working(doing) => {
            lines.push(Line::styled(pad(format!("… {doing}")), strong));
            lines.push(Line::default());
        }
        Stage::Waiting | Stage::Keep => {
            if let Some(npub) = &form.npub {
                lines.push(Line::styled(
                    pad("Your id, to send to an admin".into()),
                    strong,
                ));
                lines.push(Line::from(pad(format!("  {}", npub_line(npub, width)))));
            }
            let name = form
                .known()
                .map(|(address, facts)| facts.label(address.relay()))
                .unwrap_or_else(|| "This community".into());
            for text in [
                format!("{name} lets in its members only. Send this id to one of"),
                "its admins; once they add you, bzz comes in by itself.".to_owned(),
            ] {
                lines.push(Line::from(pad(text)));
            }
            lines.push(Line::styled(
                pad(if form.asking {
                    "asking the community…".to_owned()
                } else {
                    format!(
                        "waiting for an admin · asked {} time{} · bzz asks again on its own",
                        form.attempts,
                        if form.attempts == 1 { "" } else { "s" }
                    )
                }),
                muted,
            ));
            lines.push(Line::default());
            if form.stage == Stage::Keep {
                lines.push(Line::styled(
                    pad("Stop joining. Keep the identity made for it?".into()),
                    strong,
                ));
                lines.push(Line::from(pad(
                    "You gave its id to an admin: keep it to come back with it.".into(),
                )));
                lines.push(Line::default());
            }
        }
        Stage::Failed { what, next } => {
            lines.push(Line::styled(
                pad(format!("✗ {what}")),
                theme.style(HighlightGroup::Error),
            ));
            lines.push(Line::from(pad(format!("  {next}"))));
            if form.made_here {
                lines.push(Line::styled(
                    pad(if form.keep {
                        "  The identity made for this join stays.".into()
                    } else {
                        "  The identity made for this join is taken back; k keeps it.".into()
                    }),
                    muted,
                ));
            }
            lines.push(Line::default());
        }
        Stage::Address | Stage::Who | Stage::Demo => {}
    }

    if let Some(notice) = &form.notice {
        lines.push(Line::styled(
            pad(sanitize::single_line(notice)),
            theme.style(HighlightGroup::Warning),
        ));
        lines.push(Line::default());
    }

    let keys = match &form.stage {
        Stage::Address => "Enter continue · Esc back",
        Stage::Who => "Enter choose · ↑↓ move · Esc the address",
        Stage::Typing(Field::NewPassphrase | Field::ConfirmPassphrase) => {
            "Enter next · Tab the other field · Esc the choices"
        }
        Stage::Typing(_) => "Enter continue · Esc the choices",
        Stage::Working(_) => "Esc leave this screen; bzz goes on",
        Stage::Waiting if form.made_here => {
            "c copy your id · r ask now · Esc leave (bzz keeps asking) · x stop joining"
        }
        Stage::Waiting => "c copy your id · r ask now · Esc leave (bzz keeps asking) · x stop",
        Stage::Keep => "y keep it · n remove it · Esc go on waiting",
        Stage::Failed { .. } if form.made_here && !form.keep => {
            "Enter try again · k keep the identity · Esc leave"
        }
        Stage::Failed { .. } => "Enter try again · Esc leave",
        Stage::Demo => "Esc back",
    };
    lines.push(Line::styled(pad(keys.into()), muted));
    draw(frame, inner, lines, theme);
}

fn draw(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme.style(HighlightGroup::Normal))
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// The rows of the choice of identity for a community, in their order:
/// a new identity, the ones bzz holds, Buzz desktop, the browser where the
/// community offers it, and importing a key.
pub fn choices(
    facts: &RelayFacts,
    held: &[IdentityConfig],
    keychain: bool,
    desktop: Result<String, String>,
) -> Vec<Choice> {
    let mut rows = vec![Choice {
        who: Who::New,
        label: "A new identity, made here".into(),
        hint: kept_where(keychain).into(),
        unavailable: None,
        unlock: false,
    }];
    for identity in held {
        let npub = nostr::PublicKey::from_hex(&identity.pubkey)
            .ok()
            .and_then(|key| nostr::ToBech32::to_bech32(&key).ok())
            .unwrap_or_else(|| identity.pubkey.clone());
        rows.push(Choice {
            who: Who::Held(identity.id),
            label: sanitize::single_line(&identity.label),
            hint: format!(
                "{} · {}",
                crate::domain::abbreviated_pubkey(&npub),
                match identity.backend {
                    KeyBackend::Keychain => "an identity bzz holds, in the keychain",
                    KeyBackend::EncryptedFile => "an identity bzz holds; asks its passphrase",
                    KeyBackend::BuzzDesktop => "an identity paired with Buzz desktop",
                }
            ),
            unavailable: None,
            unlock: identity.backend == KeyBackend::EncryptedFile,
        });
    }
    rows.push(Choice {
        who: Who::Pair,
        label: "Pair with Buzz desktop".into(),
        hint: match &desktop {
            Ok(instance) => {
                format!("use the account of Buzz desktop ({instance}); approve it there")
            }
            Err(_) => "use the account of Buzz desktop on this machine".into(),
        },
        unavailable: match desktop {
            Ok(_) if !keychain => Some(
                "pairing keeps Buzz desktop's token in the system keychain, and there is none here"
                    .into(),
            ),
            Ok(_) => None,
            Err(reason) => Some(format!("{reason}: start Buzz desktop, then come back")),
        },
        unlock: false,
    });
    if facts.federated != Requirement::NotSupported {
        rows.push(Choice {
            who: Who::Browser,
            label: "Sign in through the browser".into(),
            hint: "a new identity, signed in with your company account (:signin)".into(),
            unavailable: None,
            unlock: false,
        });
    }
    rows.push(Choice {
        who: Who::Import,
        label: "Import a key".into(),
        hint: "an nsec you already have, typed or pasted here, hidden".into(),
        unavailable: None,
        unlock: false,
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::join::{RelayAddress, parse_address};
    use ratatui::{Terminal, backend::TestBackend};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_text(form: &mut JoinForm, text: &str) {
        for character in text.chars() {
            form.key(key(KeyCode::Char(character)));
        }
    }

    fn facts(members_only: bool) -> RelayFacts {
        RelayFacts {
            name: Some("My team".into()),
            description: Some("We ship things.".into()),
            relay_pubkey: "ab".repeat(32),
            members_only,
            federated: Requirement::NotSupported,
            missing_nips: Vec::new(),
        }
    }

    fn known(form: &mut JoinForm, members_only: bool) {
        let address = parse_address(&form.address).unwrap();
        form.check = Check::Known {
            input: form.address.trim().to_owned(),
            address,
            facts: facts(members_only),
        };
        let rows = choices(
            &facts(members_only),
            &[],
            form.keychain,
            Err("no running Buzz Desktop was found".into()),
        );
        form.set_choices(rows);
    }

    pub fn screen(form: &JoinForm, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), form, &Theme::default()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn typing_asks_for_a_check_after_a_pause_and_enter_goes_on_when_known() {
        let mut form = JoinForm::new(&JoinStart::Join, true);
        type_text(&mut form, "my-team.example");
        assert!(!form.check_due(Instant::now()));
        assert!(form.check_due(Instant::now() + CHECK_AFTER));
        assert_eq!(form.key(key(KeyCode::Enter)), Step::Check);
        assert!(form.continue_when_known);
        known(&mut form, false);
        assert_eq!(form.key(key(KeyCode::Enter)), Step::Nothing);
        assert_eq!(form.stage, Stage::Who);
        // The usual answer is first and chosen: with a keychain it starts.
        assert_eq!(form.choices[form.selected].who, Who::New);
        assert_eq!(form.key(key(KeyCode::Enter)), Step::Start);
    }

    #[test]
    fn without_a_keychain_a_new_identity_asks_a_passphrase_twice_hidden() {
        let mut form = JoinForm::new(&JoinStart::Address("my-team.example".into()), false);
        known(&mut form, false);
        form.key(key(KeyCode::Enter));
        assert_eq!(form.key(key(KeyCode::Enter)), Step::Nothing);
        assert_eq!(form.stage, Stage::Typing(Field::NewPassphrase));
        type_text(&mut form, "short");
        form.key(key(KeyCode::Enter));
        assert!(form.notice.as_deref().unwrap().contains("12 characters"));
        type_text(&mut form, " but now long");
        form.key(key(KeyCode::Enter));
        assert_eq!(form.stage, Stage::Typing(Field::ConfirmPassphrase));
        type_text(&mut form, "short but now lonG");
        form.key(key(KeyCode::Enter));
        assert!(form.notice.as_deref().unwrap().contains("differ"));
        type_text(&mut form, "short but now long");
        let shown = screen(&form, 80, 24);
        assert!(!shown.contains("short but"), "{shown}");
        assert!(shown.contains("••••"), "{shown}");
        assert_eq!(form.key(key(KeyCode::Enter)), Step::Start);
        assert_eq!(form.passphrase().as_str(), "short but now long");
        // Nothing of it in a Debug.
        assert!(!format!("{form:?}").contains("short but"));
        form.wipe();
        assert!(form.passphrase().is_empty());
    }

    #[test]
    fn an_import_takes_a_valid_secret_only_and_never_shows_it() {
        let keys = nostr::Keys::generate();
        let nsec = nostr::ToBech32::to_bech32(keys.secret_key()).unwrap();
        let mut form = JoinForm::new(&JoinStart::Import, true);
        form.address = "my-team.example".into();
        known(&mut form, true);
        form.key(key(KeyCode::Enter));
        assert_eq!(
            form.choices[form.selected].who,
            Who::Import,
            "the first screen's choice"
        );
        form.key(key(KeyCode::Enter));
        assert_eq!(form.stage, Stage::Typing(Field::Secret));
        type_text(&mut form, "nsec1nope");
        assert_eq!(form.key(key(KeyCode::Enter)), Step::Nothing);
        assert!(form.notice.is_some());
        form.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        form.paste(&nsec);
        let shown = screen(&form, 160, 45);
        assert!(!shown.contains(&nsec[5..20]), "{shown}");
        assert!(!format!("{form:?}").contains(&nsec[5..20]));
        assert_eq!(form.key(key(KeyCode::Enter)), Step::Start);
        // Esc back to the choices wipes it.
        form.stage = Stage::Typing(Field::Secret);
        form.key(key(KeyCode::Esc));
        assert!(form.secret().is_empty());
    }

    #[test]
    fn a_choice_that_cannot_be_made_here_says_why_and_stays() {
        let mut form = JoinForm::new(&JoinStart::Pair, true);
        form.address = "my-team.example".into();
        known(&mut form, false);
        form.key(key(KeyCode::Enter));
        assert_eq!(form.choices[form.selected].who, Who::Pair);
        assert_eq!(form.key(key(KeyCode::Enter)), Step::Nothing);
        assert!(
            form.notice
                .as_deref()
                .unwrap()
                .contains("start Buzz desktop")
        );
        assert_eq!(form.stage, Stage::Who);
    }

    #[test]
    fn the_wait_offers_the_id_to_copy_and_asks_before_dropping_what_it_made() {
        let mut form = JoinForm::new(&JoinStart::Join, true);
        form.address = "my-team.example".into();
        known(&mut form, true);
        form.stage = Stage::Waiting;
        form.made_here = true;
        form.npub = Some("npub1example".into());
        assert_eq!(
            form.key(key(KeyCode::Char('c'))),
            Step::Copy("npub1example".into())
        );
        assert_eq!(form.key(key(KeyCode::Char('r'))), Step::RetryNow);
        assert_eq!(form.key(key(KeyCode::Esc)), Step::Close);
        assert_eq!(form.key(key(KeyCode::Char('x'))), Step::Nothing);
        assert_eq!(form.stage, Stage::Keep);
        assert_eq!(
            form.key(key(KeyCode::Char('y'))),
            Step::Abandon {
                keep: true,
                leave: true
            }
        );
        let shown = screen(&form, 80, 24);
        assert!(shown.contains("npub1example"), "{shown}");
        assert!(shown.contains("lets in its members only"), "{shown}");
    }

    #[test]
    fn a_failure_takes_back_the_identity_unless_kept() {
        let mut form = JoinForm::new(&JoinStart::Join, true);
        form.stage = Stage::Failed {
            what: "my-team refused this identity".into(),
            next: "ask one of its admins".into(),
        };
        form.made_here = true;
        assert_eq!(
            form.key(key(KeyCode::Enter)),
            Step::Abandon {
                keep: false,
                leave: false
            }
        );
        form.key(key(KeyCode::Char('k')));
        assert_eq!(
            form.key(key(KeyCode::Esc)),
            Step::Abandon {
                keep: true,
                leave: true
            }
        );
    }

    #[test]
    fn describe_says_who_a_community_lets_in() {
        let address = parse_address("my-team.example").unwrap();
        assert!(describe(&address, &facts(true)).contains("members only"));
        assert!(describe(&address, &facts(false)).contains("open"));
        let local = Address::Relay(RelayAddress {
            url: "ws://localhost:3030/".into(),
            insecure: true,
        });
        assert!(describe(&local, &facts(false)).contains("without TLS"));
    }
}
