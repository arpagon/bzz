# v0.5 workspace visual-review rubric

Use a release build with a disposable bzz-owned fixture. Do not commit or share
community conversations, identities, relay URLs, or profile pictures as visual
evidence.

| Check | Pass condition |
|---|---|
| Hierarchy | One screen identifies the active community, current channel/DM, selected row, unread work, and `?` help. |
| Conversation | Author labels remain visible beside deterministic local markers; date separators and compact nearby-author grouping stay unambiguous. |
| Reading measure | A wide terminal leaves comfortable whitespace after the bounded message column; content is wrapped, never truncated. |
| Writing | The persistent dock says how to activate it, becomes the ordinary composer with `i`/click, and names a useful read-only reason when unavailable. |
| Context and Inbox | Context has an explicit close affordance; Inbox keeps clear list/detail hierarchy and returns to the selected row on narrow terminals. |
| Resize | At every supported width, hidden panes become route-local surfaces rather than unusably thin columns; dock and status never overlap conversation content. |
| Themes | Default, transparent/terminal-default, ANSI, and no-colour presentations retain textual/weight/marker cues for focus, avatars, status, and disabled writing. |
| Safety | No chrome leaks unauthorized state or causes presentation-only profile/media/network traffic. |

## Recorded review

On 2026-08-20, the owner approved the workspace appearance and interaction from
a local release build after exercising the live shell. The supplied capture
contained private community content and is intentionally not retained in this
repository or used as release artwork. Deterministic synthetic TestBackend
coverage remains the committed visual evidence; see
[`validation-v0.5.md`](validation-v0.5.md) and `tests/ui_snapshot_test.rs`.
