# bzz v0.14.0 — Buzz 1.0 era

**Status:** Release candidate. See
[`validation-v0.14.0.md`](validation-v0.14.0.md) for verification and the
items still recorded as pending.

Block released **Buzz 1.0**, a new desktop app
([`block/buzz-app`](https://github.com/block/buzz-app)), next to Buzz 0.X
([`block/buzz`](https://github.com/block/buzz)), which still hosts the relay
and the protocol. Both apps talk to the same relays. v0.14.0 makes bzz a
good neighbour to both: it applies message edits, works with Buzz 1.0 as the
app that holds your key, follows Buzz 1.0's naming and mention rules, and
syncs Solo chats and sidebar sections.

The protocol baseline moves from Buzz `f3b127e3` to
`bf3bdd0eaefe39289bf9620594f254c7f68897f5` (2026-11-13). Rust stays at
1.98.1. The plan is
[`planning/2026-11-12/v0.14.0.md`](planning/2026-11-12/v0.14.0.md).

## Highlights

- **Message edits (M1).** Edits made in any Buzz app now show: the latest
  edit's text, attachments and a quiet `(edited)` mark, in the timeline,
  threads, Inbox previews and search. Press `e` on your own message to edit
  it; its attachments are kept. Earlier bzz stored edits without applying
  them; upgrading applies the ones already in your history.
- **Buzz 1.0 as your key holder (M2).** `bzz identity pair-desktop` works with
  Buzz 1.0 as well as Buzz 0.X. Buzz 1.0 signs everyday work for other apps
  (messages, edits, reactions, status) but refuses deletes, reports, huddles,
  channel and membership changes and profile edits. bzz knows that list and
  says "Buzz does not let other apps do this; do it in Buzz" before you try,
  instead of failing halfway. Proven with a real pairing against the Buzz
  1.0.1 Linux build.
- **Names and mentions like Buzz 1.0 (M3).** People are named the way Buzz 1.0
  names them, with a short key suffix when two members of a channel share a
  name. `@` ranks channel members as Buzz 1.0 does, `Space` or punctuation
  completes an exact unique name, and a stale draft that mentions a non-member
  or more than 32 people is refused. All 146 upstream conformance cases pass.
- **Solo chats and sidebar sections (M4).** `:solo <name>` creates a Solo chat
  (a private channel just for you). `:section new|rename|delete|move|remove|up|down`
  and `:sort alpha|recent|default` edit the sidebar sections and sort that
  Buzz desktop and mobile sync, and bzz shows the ones you made there. A
  write never overwrites a sidebar document bzz cannot read.
- **Small parity (M5).** `Ctrl-o` in search loads older results. bzz sends
  `User-Agent: bzz/0.14.0` on its WebSocket connections. A relay address that
  serves no community now says so instead of retrying. Requests never carry
  an empty filter list. The local agent broker reports the relay's refusal of
  agent posts in direct messages as `unauthorized`.

## Fixes

- A private channel marked with Buzz's bare `private` tag was shown as
  public. It is now private, and an existing database is repaired the next
  time the relay sends the channel's details.
- Pairing with Buzz retries the same-user check for up to 5 s while the
  provider writes its proof (as upstream #9270 does).

## Compatibility and upgrade

- Database migrations 0016 (message edits) and 0017 (sidebar documents) run
  automatically, with the usual pre-migration backup, and the search index is
  rebuilt once. A real v0.13.0 data directory upgrades with its history,
  Inbox, search and drafts intact, and its stored edits applied
  (`scripts/upgrade-test.sh`).
- Editing needs a relay that serves edits (`effective_edit`); on an older
  relay bzz refuses to edit rather than guess.
- New dependencies: `icu_properties` 2.2 and `unicode-normalization` 0.1 for
  the name and mention rules. `quinn-udp` moves to 0.5.16 (RUSTSEC-2026-0538).
- No configuration changes.

## Security notes

Nothing new is stored and no new trust boundary is added. With Buzz 1.0 as
the provider, the key stays in Buzz; bzz checks Buzz's refusal list before
asking and remembers a refusal for the session. Sidebar documents are
encrypted to yourself (NIP-44), as Buzz writes them.

## Film

The launch film is updated for v0.14 with the new edit and Solo/sections
shots: [getbzz.dev](https://getbzz.dev). Every product shot is the real
v0.14.0 terminal on an isolated relay with generated people.

## Known limitations

- Attachments cannot be added or removed while editing.
- Buzz's starred, muted and project lists and section icons are left as Buzz
  wrote them; bzz neither shows nor edits them.
- Agents cannot be invited into a Solo chat from bzz.
- With Buzz 1.0 as the provider, the refused actions above must be done in
  Buzz, or by importing the key into bzz.
