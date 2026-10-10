# Group chats

v0.1.3 creates named rooms from any nonempty selection of existing characters.
One-member groups are supported; duplicate selections are deduplicated. A room
ID is immutable and independent of its mutable name. Canonical private room IDs
and their uniqueness survive upgrade. Membership add/remove/rejoin controls
arrive in v0.1.6; current creation records real intervals rather than placeholders.

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
one observation even when nobody is mentioned. Mentioned characters get calls immediately. In v0.1.4, unmentioned members
with a participation connection independently decide whether to volunteer; true
results admit a single ordinary persona call. Listening creates no reply. See
[participation](participation.md). A reply containing `@Name` is ordinary public text and never triggers
another call. Swift caches drafts, mention selection, errors and handles by room
ID, and renders each reply's actual author. Switching/renaming rooms cannot
redirect a call. The group header opens member private chats for settings/state
controls; those controls continue to apply to the character across rooms.

## Persona knowledge and state

Each persona retains one accepted state, native session, and ordered queue across
private chat and every group. Calls can run concurrently for different personas;
there is no ordered group scheduler in this stage. A call freezes only when its
persona reaches it. Its newly selected observations are limited to its acceptance
receipt cutoff. Previously accepted cross-room native input/output and latest
state remain available even to an older queued call. Coverage is contiguous and
does not jump over unseen events. First-ever contexts include earlier silent
observations; explicit fresh/connection boundaries still exclude earlier context.

Private messages never fan out. Group publication sends only visible reply text
and sender/room attribution to other current members. The author's native output,
reasoning and state stay internal to its history. That character can still use
its own private knowledge in public; model disclosure instructions are not a
secrecy guarantee. Nonmembers receive no events or historical backfill.

## Atomicity, intervals, and recovery

Acceptance commits canonical user message, member events, mentions, and selected
calls together. Successful completion commits validated reply, persona state,
native output, sent event and public fanout together. Failed/cancelled candidates
create none of those completion effects. A cancelled group call does not erase
any member's observation of its user message. Publication order is allocated at
acceptance, separately from placeholder display order. Intervals store a stable
ID and join/leave publication boundaries; future membership changes will deliver
future publications without reinterpreting placeholder timestamps.

Retry requires the character's latest failed/stopped/interrupted call across all
rooms and no queued/active call for that character. It reuses the original user,
room, membership and compatible frozen request; another character replying in
the same room does not supersede it. Fresh context/state/schema/connection edits
also respect persona-wide busy checks. Group Stop cancels the selected room's
oldest submission's outstanding decisions and handles; queued replies can be cancelled
individually. Other rooms and other submissions continue. Restart marks queued
and active attempts interrupted, preserving observations and accepted output
without automatic resubmission.

Schema 8 upgrades populated private data transactionally, preserves native bytes
and associations, backfills persona-wide call identity/order, and checks foreign
keys. Deterministic tests cover both protocols, publication rollback, silent
observations, stable mentions, cross-room queues/retry, abrupt restart and populated
v0.1.2 migration. Native bridge/render checks exercise real room creation,
switching, mention chips and attribution. An opt-in two-protocol endpoint check
uses only synthetic temporary roots:

```sh
PEBBLE_LIVE_API_ROOT=http://beluga:4013/v1 \
PEBBLE_LIVE_MODEL=gemma-4-31b-uncensored \
PEBBLE_LIVE_RESPONSES_SCHEMA_FORMAT=response_format \
cargo test -p pebble-core --test group_live_endpoint -- --ignored --nocapture
```

Responses schema dialect is explicit configuration; no hostname sniffing or
silent fallback occurs. Schema 9 adds durable decisions without rewriting existing
messages, memberships, native items, coverage, or state. Selective participation
is available; ordered/bounded bot chains and membership changes remain the
reviewed v0.1.5–v0.1.6 stages.
