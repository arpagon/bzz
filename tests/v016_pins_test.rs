//! Rule 19 of the v0.17.0 plan: nothing that worked stops working.
//!
//! `V016_KEYS` and `V016_COMMANDS` were generated from the code of the commit
//! v0.17.0 was cut from ("plan: v0.17.0, the first minute and everything
//! within reach", 543dbd1), by printing `KeyMap::builtin()`'s effective
//! bindings in every scope and `command::parse` of every v0.16.0 command.
//! They are never edited to make this test pass. What v0.17.0 changes on
//! purpose is listed by name in `DELIBERATE_CHANGES` and `ADDED_KEYS`, each
//! with the decision that allows it.

use bzz::ui::keymap::{KeyMap, KeyScope};

/// scope, keys, action: every effective binding of v0.16.0.
const V016_KEYS: &str = "\
Global	/	Filter
Global	1	FocusCommunities
Global	2	FocusChannels
Global	3	FocusTimeline
Global	4	FocusContext
Global	5	FocusApp
Global	:	OpenCommand
Global	?	OpenHelp
Global	Alt+h	ResizeFocusedNarrow
Global	Alt+l	ResizeFocusedWide
Global	BackTab	PreviousFocus
Global	Ctrl+d	HalfPageDown
Global	Ctrl+n	SelectNext
Global	Ctrl+p	SelectPrevious
Global	Ctrl+u	HalfPageUp
Global	Down	SelectNext
Global	Enter	ActivateFocused
Global	Esc	BackOrQuit
Global	Shift+G	JumpBottom
Global	Shift+J	ScrollViewportDown
Global	Shift+K	ScrollViewportUp
Global	Space 1	ToggleCommunities
Global	Space 2	ToggleChannels
Global	Space 4	ToggleContext
Global	Space 5	ZoomApp
Global	Space Space	ChannelSwitcher
Global	Space a	OpenContextActions
Global	Space n	OpenInbox
Global	Space o	OpenOptions
Global	Space p	ViewProfile
Global	Space r	Refresh
Global	Space s	CycleChannelSort
Global	Tab	NextFocus
Global	Up	SelectPrevious
Global	e	Edit
Global	g g	JumpTop
Global	h	PreviousFocus
Global	i	Compose
Global	j	SelectNext
Global	k	SelectPrevious
Global	l	NextFocus
Global	q	BackOrQuit
Global	r	React
Global	v	ToggleCopySelection
Global	y	CopyMessages
Workspace	/	Filter
Workspace	1	FocusCommunities
Workspace	2	FocusChannels
Workspace	3	FocusTimeline
Workspace	4	FocusContext
Workspace	5	FocusApp
Workspace	:	OpenCommand
Workspace	?	OpenHelp
Workspace	Alt+h	ResizeFocusedNarrow
Workspace	Alt+l	ResizeFocusedWide
Workspace	BackTab	PreviousFocus
Workspace	Ctrl+d	HalfPageDown
Workspace	Ctrl+n	SelectNext
Workspace	Ctrl+p	SelectPrevious
Workspace	Ctrl+u	HalfPageUp
Workspace	Down	SelectNext
Workspace	Enter	ActivateFocused
Workspace	Esc	BackOrQuit
Workspace	Shift+G	JumpBottom
Workspace	Shift+J	ScrollViewportDown
Workspace	Shift+K	ScrollViewportUp
Workspace	Space 1	ToggleCommunities
Workspace	Space 2	ToggleChannels
Workspace	Space 4	ToggleContext
Workspace	Space 5	ZoomApp
Workspace	Space Space	ChannelSwitcher
Workspace	Space a	OpenContextActions
Workspace	Space n	OpenInbox
Workspace	Space o	OpenOptions
Workspace	Space p	ViewProfile
Workspace	Space r	Refresh
Workspace	Space s	CycleChannelSort
Workspace	Tab	NextFocus
Workspace	Up	SelectPrevious
Workspace	e	Edit
Workspace	g g	JumpTop
Workspace	h	PreviousFocus
Workspace	i	Compose
Workspace	j	SelectNext
Workspace	k	SelectPrevious
Workspace	l	NextFocus
Workspace	q	BackOrQuit
Workspace	r	React
Workspace	v	ToggleCopySelection
Workspace	y	CopyMessages
Inbox	/	Filter
Inbox	1	FocusCommunities
Inbox	2	FocusChannels
Inbox	3	FocusTimeline
Inbox	4	FocusContext
Inbox	5	FocusApp
Inbox	:	OpenCommand
Inbox	?	OpenHelp
Inbox	Alt+h	ResizeFocusedNarrow
Inbox	Alt+l	ResizeFocusedWide
Inbox	BackTab	PreviousFocus
Inbox	Ctrl+d	HalfPageDown
Inbox	Ctrl+n	SelectNext
Inbox	Ctrl+p	SelectPrevious
Inbox	Ctrl+u	HalfPageUp
Inbox	Down	SelectNext
Inbox	Enter	ActivateFocused
Inbox	Esc	BackOrQuit
Inbox	Shift+G	JumpBottom
Inbox	Shift+J	ScrollViewportDown
Inbox	Shift+K	ScrollViewportUp
Inbox	Shift+U	MarkUnread
Inbox	Space 1	ToggleCommunities
Inbox	Space 2	ToggleChannels
Inbox	Space 4	ToggleContext
Inbox	Space 5	ZoomApp
Inbox	Space Space	ChannelSwitcher
Inbox	Space a	OpenContextActions
Inbox	Space n	OpenInbox
Inbox	Space o	OpenOptions
Inbox	Space p	ViewProfile
Inbox	Space r	Refresh
Inbox	Space s	CycleChannelSort
Inbox	Tab	NextFocus
Inbox	Up	SelectPrevious
Inbox	a	MarkVisibleRead
Inbox	e	Edit
Inbox	f	Filter
Inbox	g g	JumpTop
Inbox	h	PreviousFocus
Inbox	i	Compose
Inbox	j	SelectNext
Inbox	k	SelectPrevious
Inbox	l	NextFocus
Inbox	m	MarkRead
Inbox	o	OpenCanonicalContext
Inbox	q	BackOrQuit
Inbox	r	React
Inbox	v	ToggleCopySelection
Inbox	y	CopyMessages
Composer	Alt+Enter	InsertNewline
Composer	Alt+o	AttachPath
Composer	Ctrl+Left	MoveWordLeft
Composer	Ctrl+Right	MoveWordRight
Composer	Ctrl+c	ClearComposer
Composer	Ctrl+j	InsertNewline
Composer	Ctrl+k	DeleteToEnd
Composer	Ctrl+o	AttachFile
Composer	Ctrl+r	RetryAttachments
Composer	Ctrl+u	DeleteToStart
Composer	Ctrl+v	PasteClipboard
Composer	Ctrl+w	DeletePreviousWord
Composer	Delete	RemoveLastAttachment
Composer	End	MoveLineEnd
Composer	Enter	Submit
Composer	Esc	BackOrQuit
Composer	Home	MoveLineStart
Composer	Tab	Complete
Overlay	?	BackOrQuit
Overlay	Esc	BackOrQuit
Overlay	q	BackOrQuit
";

/// input, `Debug` of what `command::parse` made of it in v0.16.0.
const V016_COMMANDS: &str = "\
:lock	Lock
:reconnect	Reconnect
:resync	Resync
:theme reload	ThemeReload
:media reload	MediaReload
:inbox	Inbox
:search	Search
:dm	Dm
:dm hide	DmHide
:dm add	DmAdd
:agents	Agents(\"\")
:agents local	Agents(\"local\")
:agents directory	Agents(\"directory\")
:agent	Agent(\"\")
:agent new Scout	Agent(\"new Scout\")
:approvals	Approvals
:approval	Approvals
:approve	Approvals
:community add	AddCommunity
:community remove	RemoveCommunity
:purge-cache	PurgeCache
:system	System
:signin	SignIn
:sign-in	SignIn
:tochannel	ToChannel
:send-to-channel	ToChannel
:artifacts	Artifacts
:apps	Apps
:app	App(\"\")
:app open board	App(\"open board\")
:app close	App(\"close\")
:app zoom	App(\"zoom\")
:tasks	Tasks
:my-tasks	Tasks
:broker	Broker
:presence	Presence(\"\")
:presence on	Presence(\"on\")
:profile	Profile(\"\")
:profile npub1x	Profile(\"npub1x\")
:huddle	Huddle(\"\")
:huddle start	Huddle(\"start\")
:artifact rename X	Artifact(\"rename X\")
:status	Status(\"\")
:status clear	Status(\"clear\")
:status lunch	Status(\"lunch\")
:section	Section(\"\")
:section new X	Section(\"new X\")
:sort	Sort(\"\")
:sort alpha	Sort(\"alpha\")
:solo	Solo(\"\")
:solo X	Solo(\"X\")
:report	Report(\"\")
:report spam	Report(\"spam\")
";

/// The three changes v0.17.0 makes to what v0.16.0 did, by name.
const DELIBERATE_CHANGES: [(&str, &str); 3] = [
    (
        "d deletes my selected message",
        "plan D5: a new key in the workspace (it asks, as the actions menu does)",
    ),
    (
        "u marks unread from the selected message",
        "plan D5: a new key in the workspace (what the actions menu's \"mark unread\" does)",
    ),
    (
        ":status shows the status",
        "plan D6: a bare :status shows your status instead of clearing it; :status clear clears",
    ),
];

/// Bindings v0.17.0 adds: scope, keys, action. Every one is new (no v0.16.0
/// binding moves) and named in `DELIBERATE_CHANGES` or by its decision.
const ADDED_KEYS: &[(&str, &str, &str)] = &[];

/// Commands v0.16.0 answered with "unknown command" and v0.17.0 knows
/// (plan C4); they move nothing that worked.
const ADDED_COMMANDS: &[(&str, &str)] = &[];

const SCOPES: [KeyScope; 6] = [
    KeyScope::Global,
    KeyScope::Workspace,
    KeyScope::Inbox,
    KeyScope::Composer,
    KeyScope::Filter,
    KeyScope::Overlay,
];

fn current_keys() -> Vec<(String, String, String)> {
    let keymap = KeyMap::builtin();
    let mut rows = Vec::new();
    for scope in SCOPES {
        for binding in keymap.effective_bindings(scope) {
            rows.push((
                format!("{scope:?}"),
                binding.sequence.label(),
                format!("{:?}", binding.action),
            ));
        }
    }
    rows.sort();
    rows
}

fn pinned_keys() -> Vec<(String, String, String)> {
    V016_KEYS
        .lines()
        .map(|line| {
            let mut fields = line.split('\t').map(str::to_owned);
            (
                fields.next().unwrap(),
                fields.next().unwrap(),
                fields.next().unwrap(),
            )
        })
        .collect()
}

#[test]
fn every_v016_key_does_what_it_did() {
    let current = current_keys();
    for pinned in pinned_keys() {
        assert!(
            current.contains(&pinned),
            "{} {} no longer does {}",
            pinned.0,
            pinned.1,
            pinned.2
        );
    }
}

/// Nothing is bound that is neither v0.16.0's nor named here. A key added by
/// v0.17.0 is listed in `ADDED_KEYS` (in Workspace, a key of Workspace
/// appears once; Global bindings appear in every scope that inherits them).
#[test]
fn every_binding_is_v016s_or_named() {
    let pinned = pinned_keys();
    let added = ADDED_KEYS
        .iter()
        .map(|(scope, keys, action)| {
            (
                (*scope).to_owned(),
                (*keys).to_owned(),
                (*action).to_owned(),
            )
        })
        .collect::<Vec<_>>();
    for binding in current_keys() {
        assert!(
            pinned.contains(&binding) || added.contains(&binding),
            "{binding:?} is new: name it in ADDED_KEYS with its decision"
        );
    }
    for binding in &added {
        assert!(
            !pinned
                .iter()
                .any(|old| old.0 == binding.0 && old.1 == binding.1),
            "{binding:?} replaces a v0.16.0 key"
        );
    }
}

#[test]
fn every_v016_command_parses_as_it_did() {
    for line in V016_COMMANDS.lines() {
        let (input, parsed) = line.split_once('\t').unwrap();
        assert_eq!(
            format!("{:?}", bzz::ui::command::parse(input)),
            parsed,
            "{input}"
        );
    }
    for (input, parsed) in ADDED_COMMANDS {
        assert_eq!(
            format!("{:?}", bzz::ui::command::parse(input)),
            *parsed,
            "{input}"
        );
        assert!(
            !V016_COMMANDS
                .lines()
                .any(|line| line.split('\t').next() == Some(*input)),
            "{input} was a v0.16.0 command"
        );
    }
    assert_eq!(DELIBERATE_CHANGES.len(), 3);
}
