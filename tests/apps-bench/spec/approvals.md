# App spec: Approvals

Requests that are pending, approved or rejected. Anyone files a request;
anyone except its author approves it, or rejects it with a reason. A decided
request stays decided. Every participant sees other participants' requests
and decisions without reloading. The bench (`run --app approvals`)
checks only what this page fixes; anything not fixed here (colours, borders,
spacing, extra hints) is free.

## Start, data and exit

- The app is one script written against `docs/app-api.md`, `api = 1`. The
  bench runs it on `examples/app_host.rs`, which lends it the terminal and
  turns the data service of `data-protocol.md` into the contract's records.
- A request is a record of type `request` with content
  `{"title": string, "author": string, "state": "pending" | "approved" |
  "rejected", "decided_by": string | null, "reason": string | null,
  "order": number}`. `author` and `decided_by` are participants' ids; the
  local participant's is `ctx.user.id` (v0.16 M4: the template stores ids;
  the bench's host makes the id of `ana` from her name). Wherever a person
  is shown below (`<name>`), it is `host.name` of the id.
- Load with one `data.list` with `type = "request"`; show requests in the
  order of their `order` field (a list arrives in no order; a new request
  gets an order that puts it last, `host.time()`). Other participants'
  changes arrive as pushes; never poll.
- "Anyone except its author" is a convention between honest participants:
  any member of the channel can write any field of any record, and the app
  only declines to do so itself.
- `q` (when no form is open) closes the app with `host.quit()`.
  `Ctrl+C` never reaches the script: the host leaves on it. Either way,
  even with requests still pending, the bench expects of the host: exit
  within 2 s with status 0, no child process left, and the terminal
  restored (echo and line mode back, cursor visible, bracketed paste, mouse
  and focus reporting off, alternate screen left).

## Screen

All text below is exact (ASCII except user data, case-sensitive).

- **Filter indicator**: `Filter: all`, `Filter: pending`, `Filter: approved`
  or `Filter: rejected`.
- **Request rows**, in this order: the selection marker (`>` if selected,
  else a space), the state (`pending`, `approved` or `rejected`), one or more
  spaces, the title. More may follow on the row (for example the author).
  Example: `> pending  New laptop`. Titles of up to 16 characters are shown in
  full at any size.
- If the filter shows no request: `No requests match.`.
- If there are more requests than fit, show a contiguous run that includes
  the selected request.
- **Detail** of the selected request, one line each: `Request: <title>`,
  `Author: <name>`, `State: <state>`; when decided also `Decided by: <name>`;
  when rejected also `Reason: <reason>`. Reasons of up to 24 characters are
  shown in full.
- **Status line**: the last row of the screen. Exact texts:

| When | Status line contains |
| --- | --- |
| A load is pending | `Loading...` |
| A load succeeded | `Loaded N requests.` (N = records returned) |
| A load failed | `Load failed. Press r to retry.` |
| A write is pending | `Saving...` |
| A request was filed | `Request filed.` |
| You approved / rejected | `Approved.` / `Rejected.` |
| `a` or `x` on your own request | `You cannot decide your own request.` |
| `a` or `x` on a decided request, or someone decided it first | `Already approved by <name>.` / `Already rejected by <name>.` |
| A write failed (not a conflict) | `Save failed.` |

- Nothing may be written outside the screen: no line wider than the terminal
  (no autowrap) and no scrolling of text off the top.
- **Narrow screen**: the app is fully usable at 40x16 and at any larger size:
  the filter indicator, at least five request rows (or all of them), the
  status line, the detail and both forms fit, and every key works. Never show
  a "terminal too small" screen at 40x16 or larger. Every size change is
  followed by a redraw at the new size, with the same selection and data.

## Selection and keys

Initial selection: the first request.

| Key | Action |
| --- | --- |
| `Up`/`k`, `Down`/`j` | Previous / next shown request (stops at the ends) |
| `Enter` | Show the detail of the selected request |
| `Esc` | Close the detail |
| `n` | Open the new-request form |
| `a` | Approve the selected request |
| `x` | Reject the selected request (opens the reason form) |
| `f` | Cycle the filter: all, pending, approved, rejected, all... |
| `r` | Reload (`data.list` again) |
| `q` | Quit |

The detail may replace the list until `Esc`, or (at 80 columns or more) be
shown beside it at all times. While it is shown, `j`/`k`, `a`, `x`, `f`, `n`,
`r` and `q` keep working and the detail follows the selection.

**Deciding.** `a` on a pending request by someone else sends one
`data.update` with its current `rev` and its whole content with
`"state": "approved"`, `"decided_by": <ctx.user.id>` (its title, author,
`order` and a `null` reason are kept); `Saving...`, then the row shows `approved`
and the status `Approved.`. `a` or `x` on your own request sends nothing and
shows `You cannot decide your own request.`; on a decided request it sends
nothing and shows `Already <state> by <decided_by>.`; `x` then opens no form.

## Forms

Both forms take keys while open (`q`, `j`, `a`, `x` are text); `Backspace`
deletes one character; any Unicode text can be typed and is shown as typed.
Each shows the hint `Enter save  Esc cancel`. `Esc` closes the form, sends
nothing and returns to the list with the selection it had.

- **New request**: heading `New request`, field `Title: `. `Enter` with an
  empty or blank title shows `Title is required.` and sends nothing.
  Otherwise one `data.create` of type `request` with `{"title": <trimmed
  title>, "author": <ctx.user.id>, "state": "pending", "decided_by": null,
  "reason": null, "order": <a number that puts it last>}` (a `null` is
  written `data.null`). On success the form closes, the new request is selected
  and the status is `Request filed.`.
- **Reject**: heading `Reject request`, field `Reason: `. `Enter` with an
  empty or blank reason shows `Reason is required.` and sends nothing.
  Otherwise one `data.update` with `"state": "rejected"`, `"decided_by"` and
  `"reason": <trimmed reason>`. On success the form closes and the status is
  `Rejected.`.
- **One confirmation, one write**: while a write is pending (`Saving...`),
  `Enter` in a form, `a` and `x` are ignored.

## Filter

`f` cycles the filter at once, without contacting the service. Selection with
a filter: if the selected request is still shown it stays selected; otherwise
the first shown request is selected. Going back to `all` keeps the selected
request selected. Remote changes respect the filter.

## Paste

Bracketed paste is one piece of text, never keys. In a form field it is
inserted with each line break (CR, LF or CRLF) replaced by one space. With no
form open, a paste is ignored.

## Loading, errors and other participants

- **Slow load**: while the first load is pending no request rows are shown and
  the status is `Loading...`; `q` and `Ctrl+C` still quit.
- **Load error**: show `Load failed. Press r to retry.`, keep everything shown
  before, and keep the selection. `r` retries.
- **Late load**: a reply to `data.list` can arrive after a change that is
  newer than it (your own write's reply, or a push). The contract has no
  counter to compare, so remember which records changed since the list was
  asked for, and keep those as you have them when its reply arrives: a
  slow reload must never undo a newer decision.
- **Remote change** (push): apply at once, without a reload. New requests
  appear last, decisions change the state, deleted requests disappear. The
  selected request stays selected while it is shown.
- **Decided first by someone else** (reply `conflict` to your decision): take
  `current` from the reply. If its state is no longer `pending`, send nothing
  more: show `current` and `Already <state> by <decided_by>.`. Never
  overwrite another participant's decision. If it is still `pending`, apply
  your decision to `current.content` and send again with `current.rev`.
