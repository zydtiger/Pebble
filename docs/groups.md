# Group chats

v0.1.3 creates named rooms from any nonempty selection of existing characters.
One-member groups are supported; duplicate selections are deduplicated. A room
ID is immutable and independent of its mutable name. Canonical private room IDs
and their uniqueness survive upgrade. v0.1.6 adds native **Manage members** controls to add, remove or rejoin existing
characters. A room may become empty without losing its identity or transcript.

## Mentions and room presentation

Rust resolves mentions inside the user-acceptance transaction. Typed `@Name`
uses the current full member name, ending at whitespace/punctuation. Names with
spaces and Unicode are supported. Email addresses embedded inside a word are
ordinary text. Unknown and ambiguous names reject the submission before any
message/event/call commits. Duplicate names require removing the ambiguous token
and choosing a member explicitly. The native picker binds stable persona IDs;
chips remain visibly selected while the body is edited, and can be removed
independently. Chosen labels are included in the canonical saved text. Typed and
picked targets are deduplicated and stored with the current membership interval.
Rename never changes a previously accepted target.

Every user message is stored once in its room. All current characters receive
one observation even when nobody is mentioned. User mentions become protected
required slots; unmentioned members with a participation connection independently
decide whether to volunteer. The room admits only its next ready speaker, creating
that call and bubble then. Listening creates no reply. See [participation](participation.md)
and [room scheduling](group-flow.md). A bot reply can prompt bounded followup
classification, but its `@Name` text never forces a speaker. Swift caches drafts, mention selection, errors and handles by room
ID, and renders each reply's actual author. Switching/renaming rooms cannot
redirect a call. The group header opens member private chats for settings/state
controls; those controls continue to apply to the character across rooms. Drafts
and selected mentions live until the app closes. Acceptance clears them only if
no edit occurred in the meantime, even when an edit restores identical text or
selection. Removal prunes only that member's unaccepted selected chip; rename
changes its label without changing its ID.

## Persona knowledge and state

Each persona retains one accepted state, native session, and ordered queue across
private chat and every group. Different rooms/private chats can run concurrently; each group releases speakers
in its durable room order without reserving future persona work. A call freezes only when its
persona reaches it. Its newly selected observations are limited to its acceptance
receipt cutoff. Previously accepted cross-room native input/output and latest
state remain available even to an older required slot. Explicit accepted
same-round predecessors are also eligible without widening the original cutoff.
Coverage is contiguous and
does not jump over unseen events. First-ever contexts include earlier silent
observations; explicit fresh/connection boundaries still exclude earlier context.

Private messages never fan out. Group publication sends only visible reply text
and sender/room attribution to other current members. The author's native output,
reasoning and state stay internal to its history. That character can still use
its own private knowledge in public; model disclosure instructions are not a
secrecy guarantee. Nonmembers receive no events or historical backfill.

## Atomicity, intervals, and recovery

Acceptance commits canonical user message, member events, mentions, round,
decisions and slots together. Admission separately creates only the ready call. Successful completion commits validated reply, persona state,
native output, sent event and public fanout together. Failed/cancelled candidates
create none of those completion effects. A cancelled group call does not erase
any member's observation of its user message. Publication order is allocated at
acceptance, separately from placeholder display order. Intervals store a stable
ID and join/leave publication boundaries. Join/rejoin receives only subsequent
publications, including an older placeholder that completes after joining, with
no backfill before joining or while absent. Newly joined members become response
candidates at the next user round. Removal closes the interval and cancels its
unfinished decisions, slots and calls atomically under the publication gate;
late callbacks cannot accept state, native output or fanout. Rejoin creates a new
interval and cannot revive old calls or make their Retry eligible. Accepted output
and previously observed knowledge remain.

Retry requires the character's latest failed/stopped/interrupted call across all
rooms and no queued/active call, pending decision or still-admittable slot for that character. It reuses the original user,
room, membership and compatible frozen request; another character replying in
the same room does not supersede it. Fresh context/state/schema/connection edits
also respect persona-wide busy checks. Native sends steer unfinished replies into a new round with the latest input.
Shared personas' other-room/private calls continue. Native steering carries outstanding required speakers forward; lower-level
enqueues preserve their original required slots. Restart interrupts
unfinished calls/decisions/slots/rounds, preserving observations and accepted
output without automatic resubmission. Explicit Retry never restarts a chain.

Schema 8 upgrades populated private data transactionally, preserves native bytes
and associations, backfills persona-wide call identity/order, and checks foreign
keys. Deterministic tests cover both protocols, publication rollback, silent
observations, stable mentions, cross-room queues/retry, abrupt restart and populated
v0.1.2 migration. Native bridge/render checks exercise real room creation,
switching, mention chips, member management and attribution. Both-protocol
crash fixtures reopen during classification, queueing and generation, preserving
accepted native bytes/state and interrupting unfinished work without resending. An opt-in two-protocol endpoint check
uses only synthetic temporary roots. Replace the example API root and model
with your deployed service:

```sh
PEBBLE_LIVE_API_ROOT=https://api.example.com/v1 \
PEBBLE_LIVE_MODEL=gemma-4-31b-uncensored \
PEBBLE_LIVE_RESPONSES_SCHEMA_FORMAT=response_format \
cargo test -p pebble-core --test group_live_endpoint -- --ignored --nocapture
```

Responses schema dialect is explicit configuration; no hostname sniffing or
silent fallback occurs. Schema 9 adds durable decisions without rewriting existing
messages, memberships, native items, coverage, or state. Selective participation
is available. Schema 10 adds ordered rounds/slots and generation-scoped decisions
without rewriting existing native history. Membership editing reuses these
intervals and needs no further schema migration.
