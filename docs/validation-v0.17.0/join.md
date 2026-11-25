# v0.17.0 — join: joining a community without leaving bzz (A9)

Linux x86_64, 2026-11-25, branch `t3/implement-join-command` cut from
`v0.17.0` at `69fdec2`. Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`.
Plan: [`docs/planning/2026-11-24/v0.17.0.md`](../planning/2026-11-24/v0.17.0.md),
item A9, rules 19, 20 and 22, and the privacy and key rules of v0.13 to
v0.15 (4 and 10). Method:
[`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).
`<scratch>` is a folder on the machine the work was done on, outside the
repository. Every identity below was generated for the run; no keychain,
Buzz desktop or account of this machine was used.

## What was built

| Item | Where |
|---|---|
| The flow's logic, shared by the CLI and the form: the address and invite links read, NIP-11 probed and told apart, the AUTH answer told apart, an invite claimed, a community written with its relay key pinned | `src/join.rs` |
| Pairing with Buzz desktop, lifted from the CLI | `src/auth/pairing.rs` |
| `IdentityManager::save_new` (write an identity, take it back on failure), `forget` (take back one this join made), `keychain::available` (asked by reading an entry that does not exist) | `src/auth/mod.rs`, `src/auth/keychain.rs` |
| The CLI's `community add`, `identity new`, `import`, `import-backup`, `pair-desktop` call them | `src/main.rs` |
| The form: its state, keys and screen; secrets in `Zeroizing` buffers drawn as dots | `src/ui/join.rs` |
| The App's half: the seam `open_join(JoinStart)`, background steps, the wait and its pace, the landing, the demo seam | `src/app/join.rs` |
| `:join [address]`, `:community add [address]` | `src/ui/command.rs`, `src/app.rs` (`execute_command`) |
| The switch to a runtime, shared by switching communities and joining | `src/app.rs` (`adopt_runtime`) |
| Three tmux scenarios, and the runner's `empty` and `isolated` | `scripts/tui-herdr-scenarios.toml` (end), `scripts/test-tui-tmux.py`, `scripts/test-tui-herdr.sh` |

## Claims

```sh
cargo test --locked --lib join            # 27 tests: src/join.rs, src/ui/join.rs, src/app/join.rs
cargo test --locked --test join_test      # 5 tests on a scripted relay
BZZ_BIN=<release bzz> BZZ_TMUX_SCENARIOS=t17-join-not-there ./scripts/test-tui-tmux.py
BZZ_TMUX_SCENARIOS=t17-join-open ./scripts/test-relay.sh --tmux
BZZ_FIXTURE_AGENTS=0 BUZZ_REQUIRE_RELAY_MEMBERSHIP=true RELAY_OWNER_PUBKEY=<64 hex> \
  BZZ_TMUX_SCENARIOS=t17-join-members ./scripts/test-relay.sh --tmux
```

| Claim | Lowest rung | Proof | Result |
|---|---|---|---|
| From an empty home with no keychain, a person joins the pinned relay's open community, lands, opens a channel and sends a message, with nothing typed in a shell but `bzz` | the release binary on the pinned relay, through tmux | `t17-join-open`: `:join localhost:3030` → `✓ Buzz Relay · a Buzz community at localhost:3030` / `open: anyone who signs in may join` → Enter → `A new identity, made here · no keychain: key in a file with a passphrase` → the passphrase twice, drawn as dots → `choose a channel to open` (the finder on the open channels) → `t9-gen` Enter → `i`, the message, Enter → the message in `#t9-general`, and no longer `[pending]` (acknowledged by the relay) | passed, 20 steps, 3.3 s |
| Against a relay that lets in members only, the npub is shown; the screen is left and found again; the person is let in from the harness side; bzz gets in by itself without a restart | the same, relay with `BUZZ_REQUIRE_RELAY_MEMBERSHIP=true` (NIP-43 advertised) | `t17-join-members`: `it lets in its members only` → `Your id, to send to an admin` and a full `npub1…` (58 characters checked) → `waiting for an admin · asked 1 time` → Esc: `the join goes on; :join shows it again` → `:join`: the same wait → a row in `relay_members` for the pubkey bzz wrote in the profile's settings (`docker compose … psql`, as the fixture seeds members) → with no key pressed, `choose a channel to open` 14.7 s later (the second ask, 15 s after the first) → a message sent and acknowledged | passed, 11 steps, 18.2 s |
| The relay's AUTH answer is told apart: in, not a member, refused (a ban), sign-in first, no answer | scripted relay | `join_test::the_relays_answer_to_auth_decides_what_comes_next`; `join::tests::a_refused_auth_is_told_apart` | pass |
| No answer at the address; a relay with no community there (404); something that is not a Buzz relay (a plain relay with no signing key, a web page): each says what happened and what to try | scripted relay, real closed port | `join_test::each_failure_at_the_address_is_told_apart`; in the form `app::join::tests::an_address_where_nothing_answers_says_so_in_the_form`; through tmux `t17-join-not-there` (`✗ nothing answers at localhost:9` / `check the address with the person who gave it to you…`; `ws://far.example` → `ws:// and http:// work only on this machine; use wss://`) | pass |
| An identity refused: said in the form, and the identity this join made is taken back (`k` keeps it) | App on a scripted relay that answers `blocked: you are banned` | `app::join::tests::a_refused_identity_is_said_and_nothing_half_made_stays`: the form says `My team refused this identity (blocked: you are banned)`; after Enter the settings hold no community and no identity | pass |
| Nothing half-made: no community entry is written before the relay lets the identity in; stopping a wait asks whether to keep the identity whose id was given out, and `n` removes it and its key file | App, scripted relay | `a_members_only_community_shows_the_npub_and_lets_in_without_a_restart` (no community while waiting; one identity); `stopping_a_wait_keeps_or_drops_the_identity_as_the_person_says` (both answers; the key files counted) | pass |
| The wait asks again on its own at a calm pace (15 s, 30 s, then every minute) and on `r`, and asks nothing before its time | App, scripted relay | `the_wait_asks_again_on_its_own`; `retry_after`; `the_members_only…` (`r`); on the pinned relay, the 14.7 s above | pass |
| An invite link of the pinned Buzz (`https://<relay>/invite/<code>`, `buzz://join?relay=…&code=…`) is read, claimed with NIP-98 by the joining key with the code in the body only, and lets the identity in; its refusals are Buzz desktop's words | scripted relay | `join_test::an_invite_is_claimed_by_the_joining_key_with_the_code_in_the_body_only`; `app::join::tests::an_invite_link_is_claimed_and_lets_in_a_members_only_community`; `join::tests::invite_links_of_the_pinned_buzz_are_read` | pass. Not run against the pinned relay's `/api/invites/claim` (an invite needs an admin's mint) |
| The CLI and the form write a community the same way: probe, then `add_community` (configuration, store row, relay key pinned); a relay already configured is refused and nothing changes | integration | `join_test::a_community_is_written_with_its_relay_key_pinned_and_taken_back_on_failure`; the CLI's own tests in the gates | pass |
| `:community add` opens the form (it answered "use the CLI"); `:join` and `:join <address>`, `:community add <address>` too; a paste goes into the field as one piece of text | App | `the_community_add_command_opens_the_form_and_a_bad_address_is_said`; `ui::command` tests | pass |
| After joining, bzz opens the first channel the person is in, or the finder on the open ones; nothing is joined for them | App; pinned relay | `after_joining_bzz_lands_in_the_first_channel_or_offers_the_open_ones`; both relay scenarios | pass |

### Secrets

A passphrase and an nsec are typed only into the form's hidden fields, as
`bzz identity new` and `import` read them only from the controlling
terminal. They are held in `Zeroizing` buffers, wiped when the form moves
on or goes back, passed to `IdentityManager` as a `SecretString` or as
`Keys`, never put in a notice, a `Debug` (the form's `Debug` is written by
hand) or a diagnostic. An invite's code is a bearer of an admin's
permission: drawn as dots, out of `Debug`, sent only in the claim's body to
its relay.

How it was checked:

- On screen: `ui::join::tests::without_a_keychain_a_new_identity_asks_a_passphrase_twice_hidden`
  and `…an_import_takes_a_valid_secret_only_and_never_shows_it` render the
  form at 80×24 and 160×45 and find dots and not the secret, and not in
  `Debug`; `an_invite_code_is_drawn_as_dots`.
- In every file bzz wrote: `app::join::tests` read every file under the
  test's home after a join (settings, key file, database) and find neither
  the passphrase nor the invite's code.
- In the process and the terminal, on the pinned relay: `t17-join-open`
  step 8 reads `/proc/<pid>/cmdline` and `/proc/<pid>/environ` of the
  profile's bzz (found by its `BZZ_CONFIG_DIR`) while the passphrase is
  typed; step 14 greps the profile's whole home (settings, the key file,
  the database, diagnostics, cache) and the bytes bzz wrote to the
  terminal (the pane's pipe). The passphrase is in none of them.
- What is not bzz: the scenario types the generated passphrase with
  `tmux send-keys`, one key per argument, as a person types it, and the
  check's own `grep` has it as its pattern. Both are the harness, with a
  passphrase generated for the run.

### The demo (rule 22)

Under `bzz demo` the form opens and says that joining needs a real
community's address, that the demo writes nothing to the person's
settings, and to leave with `q` and start `bzz`. It writes nothing. The
demo is agent `demo`'s branch (`t3/demo`), not this one: `open_join` asks
`join_in_demo()`, which returns `false` here and is to return `self.demo`
when the two are joined (`JoinForm::demo` is the screen).

### Pairing with Buzz desktop

Not run with a desktop: no Buzz desktop may be started on this machine.
The form's choice calls `auth::pairing::find_desktop` and `pair`, the
same two functions `bzz identity pair-desktop` now calls, and the CLI's
own tests go as far as the provider (`tests/credential_provider_test.rs`,
`pairing_needs_no_token_and_returns_one`, a fake provider). Shown by
tests: without a running desktop the choice says `Buzz desktop is not
running here: start it, then come back` and stays; without a keychain it
says pairing needs one (the paired token lives only there).

What a person must check, with Buzz desktop running on the machine and a
keychain: `:join <address>`, choose "Pair with Buzz desktop", approve the
request in Buzz, and see bzz come in with the desktop's npub.

### Sign-in through the browser

Built, not run on a NIP-FI relay: where NIP-11 says the community signs in
through an identity provider, the form offers it, asks the address of the
sign-in service (https, as `identity_adapter` is configured; ADR v0.13:
bzz never derives it from relay data), makes a new identity, writes the
community with that adapter and runs `:signin`. Tested at the form's rung
only (`the_browser_is_offered_where_the_community_signs_in_through_it`).

## The first minute, counted

The study (`r4-first-run/REPORT.md` §3) counted five commands after the
installer and ten ideas to know before: identity, label, keychain,
backend, passphrase, identity id, community, relay, `wss://` address,
pubkey/npub, membership.

| | Commands in a shell | Ideas the person needs |
|---|---|---|
| Before | 5 (`identity new`, `--backend`, `identity list`, `community add`, `bzz`) | 10 |
| Now, an open community, with a keychain | 1 (`bzz`) | 1: the community's address |
| Now, no keychain | 1 | 2: and a passphrase |
| Now, members only | 1 | 3: and "your id, to send to an admin" |

Keys after `bzz`: `:join` and the address (or `j` on the first screen once
it is joined with agent `demo`'s), Enter, Enter for a new identity, the
passphrase twice when there is no keychain, Enter on a channel.

## Captures

Text captures of every step from tmux, at 80×24 and 160×45, in
`<scratch>/captures-open/t17-join-open/`,
`<scratch>/captures-members/t17-join-members/` and
`<scratch>/captures-notthere/t17-join-not-there/` (one file per step; its
first line says the keys and the size).
