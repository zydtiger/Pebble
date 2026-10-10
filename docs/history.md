# Durable chat history

v0.1.5 uses one SQLite database and live Chat Completions/Responses adapters. An
unconfigured character uses a deterministic local demo. Groups share persona histories. Independent participation decisions do not
create native sessions or consume event coverage. Durable room rounds release
only ready slots into the persona queues; no memory writer or work-agent integration exists.

## Records and ordering

| Table | Identity, order, and responsibility |
| --- | --- |
| `personas` | Stable UUID and directory; display name is derived from files |
| `conversations` | One canonical `direct` room per persona plus named `group` rooms; existing IDs survive migration |
| `messages` | Visible content, explicit user/persona sender, status, timestamp; unique `(conversation_id, sequence)` for display; separate nullable publication order assigned only on acceptance |
| `persona_events` | Received eligible public messages and accepted own sent replies; unique `(persona_id, sequence)` and `(persona_id, message_id)` |
| `model_sessions` | One active context per persona; protocol/model/API-root binding, event start boundary, covered-event frontier, and frozen response schema (NULL for preserved legacy sessions) |
| `model_items` | Frozen raw/zstd prefix/input/output JSON BLOBs with decoded length; unique `(session_id, sequence)`; optional producing call |
| `model_item_events` | Explicit item/event links, including own outputs; indexes support reverse lookup |
| `persona_states` | Persona-wide current accepted state; independent of model session |
| `conversation_memberships` | Durable group/persona intervals, join/leave publication boundaries, stable interval IDs and member order |
| `message_mentions` | Canonical user message, resolved persona ID and originating membership interval; deduplicated per message/persona |
| `participation_decisions` | User-root/persona/member interval, original receipt cutoff and revision, pending/boolean/error/cancel/restart status, optional admitted call ID, phase, generation, accepted-knowledge/predecessor versions and elapsed milliseconds; unique `(message_id,persona_id)`. No copied classifier payload or private/native history. |
| `group_rounds` | Canonical user root/room/revision, active/superseded/terminal lifecycle, continuation, spent followup budget, last accepted public reply and fallback phase |
| `group_reply_slots` | Round/decision identity and fixed ordinal, required/optional/followup/fallback kind, pending/admitted/terminal status; no model call before readiness |
| `model_calls` | Immutable room/persona/membership identity and persona-wide call order; queued/active lifecycle, optional session/input cutoff, reply room, trigger/reply IDs, receipt/event cutoff, status/error, optional originating slot and retry_of, frozen state_before, and validation_errors |

All sequences are assigned inside transactions. Timestamps describe creation;
they do not reorder stored history. A message ID identifies the same bubble
while streaming and after restart. The runtime owns the reply destination.
One session belongs to a persona, not an app launch or visible room.

The first input initializes a session with a fixed application/SOUL/MEMORY
snapshot. A first-ever session includes all prior received observations up to the
call cutoff; there is no group backfill to nonmembers. Reloading files changes the character panel, but does not rewrite
that existing snapshot. Sessions identify `pebble_demo_v1` / `local-demo` or
`chat_completions` or `responses` / the configured model. The credential-free `base_url` is
stored on the session; keys are resolved at runtime and never persisted.

Changing the API root, model, or protocol retires the previous session. The new
session's `event_start_sequence` is immediately before the triggering received
event. Its requests include later received events in execution order; earlier
sent events stay in their original native sessions. This fresh context does not import old demo
or provider output. Old visible messages and frozen items remain preserved. A
notice is derived from the stored boundary and is visible after restart too.
An explicit fresh-context command saves a new prefix and boundary immediately,
using current character files. It refuses queued/active calls, pending classifier
decisions and still-admittable group slots for that persona;
other personas are independent. The manual boundary and its notice survive
restart, including a boundary before any received event. No earlier received
or native sent event is reimported.

## One turn

```text
send(persona, text)
  transaction
    user message U1 + streaming reply placeholder A1
    received event E1 + queued call C1 (receipt cutoff; no session/input yet)
  commit
  wait for this persona's preceding call to reach terminal state
  activation transaction
    session S (resolve or create fixed prefix P)
    frozen input I1 + link I1 -> E1
    activate C1 with session/input/event cutoffs
  commit
  load C1's items from S ordered by sequence: P, I1
  live adapter (or demo) reads that stored request outside the transaction
  buffer candidate text privately; validate JSON/schema
  regenerate invalid format/schema at most twice from the same frozen input/state
  completion transaction
    independently validate; finalize A1.reply + persona state + native output items O1…On
    sent event E2 + link each O1…On -> E2
    finalize C1 and advance contiguous event coverage
  commit, then notify completion

next turn's request: P, I1, O1…On, I2
```

The input item is JSON with `role: user` and `content` containing a serialized
envelope. Its `events` include source event/message/room IDs, event sequence,
speaker kind/ID/name, and text. `reply_request` contains the target room and trigger
message, plus the current destination kind/name/member attribution. These are fields inside content, not imaginary provider API fields.
The prefix defines this attribution. Participant text is serialized as data.
Chat Completions stores one native assistant message with supported
`reasoning_content`/`refusal` fields. Responses input uses native `message` items
with `input_text` content; accepted output stores every whole native reasoning
and assistant message item separately, in provider order. IDs, encrypted state,
and unknown fields are retained without reconstructing reasoning. All output
items link to the same sent event in one transaction. Only validated `reply` is rendered; the full native JSON retains state and reasoning.

The builder appends events without a representation in the active session.
Received events later than this call's acceptance cutoff are excluded, except
explicit accepted earlier-slot public replies in its own round. This selective
exception preserves coverage holes instead of raising the cutoff. Queued calls freeze only
when executing, so the predecessor's native output is already in the prefix.
Own sent events enter a session only through their native output links.
Unlinked sent events from an earlier connection are excluded from catch-up and
do not block the frontier; they never become incoming user messages. The covered frontier stops before the first
unrepresented event instead of jumping to the largest linked sequence.

A call records the last input item sequence. Reconstructing that call loads
items through this cutoff, retaining the exact saved bytes. It excludes its
own future output and later turns. There is no full-request copy per call,
transcript re-rendering, dynamic timestamp in the prefix, or server KV cache
management. v0.0.6 stores each item as raw JSON or lossless zstd. The loader checks
aggregate decoded size before materializing history and checks a conservative
input estimate plus output reserve before HTTP. See [context and encoding](context.md).

## Failure and lifetime boundaries

Acceptance and final publication are separate atomic transactions. A failure
cannot publish half an output/event association. Structured candidate deltas are not checkpointed; legacy plaintext contexts retain
visible partial text; only successful completion creates native model
items and a sent event. Stopped/failed/interrupted bubbles are explicit in the
UI and do not masquerade as accepted assistant context. The accepted user
input stays in the session even when its reply is cancelled.

One worker runs per persona at a time; different personas run concurrently.
Queued replies are typed `queued` snapshots derived from their call state.
Cancelling a queued call leaves visible messages and its received event, with a
terminal call and no session/model input. Private-chat builders explicitly skip these unconsumed cancelled/interrupted
inputs; the covered frontier can pass them. Group inputs remain public observed
facts and are still consumed by later calls, including silent members. A different
member's cancelled call never suppresses an observation.
Rust owns cancellation and durable terminal states; Swift consumes typed message
snapshots and output deltas.
The draft is cleared after Rust accepts it, and callback IDs prevent updating
the wrong visible message. Character switching keeps each room
stream and unsent draft alive; updates reconcile only the matching room.
Dropping a handle or disconnecting its observer cancels the worker. The UI
reloads the final transcript from Rust. No failed-call resubmission is automatic; only completed invalid structured candidates get bounded format repair. Latest-reply retry is persona-wide across rooms, uses the immutable original
destination/membership, and requires that persona idle, including pending
decisions/slots. Retry never restores automatic round continuation/budget. It adds a new reply/call
referencing retry_of and the existing user/receipt. In the unchanged active
context it reuses the nearest frozen ancestor's input/event cutoff. An
unactivated retry admits that receipt once; explicit retry after a fresh or
changed connection can select its original event before the boundary, without
general catch-up. Subsequent turns retain its native links and do not duplicate
it. Earlier attempts, partial text, and original call cutoffs stay unchanged.

An advisory lock on `data/app.lock` reserves the data root for the core and any
worker still using its database. The file contains no chat state; OS ownership
is released on normal or abrupt exit. A second opener fails without touching
running calls. After acquiring ownership, startup transactionally marks any
remaining queued or streaming replies/calls interrupted, preserving their durable
text and accepted input. It never invents a completed model output. This is
restart consistency. Snapshots expose durable call errors and latest-reply retry
eligibility; an interrupted queued call can be explicitly retried after reopen.

Schema 1 upgrades to 2 by adding the six history tables and indexes. Schema 2
upgrades to 3 with credential-free API-root binding and an event-start boundary.
Schema 3 upgrades to 4 by rebuilding model_calls with a queued status and
nullable session/input cutoff until activation. Migration preserves all IDs and
payloads, checks foreign keys before commit, and restores enforcement on the
runtime connection. Schema 4 upgrades to 5 by rebuilding model_items to allow
json/zstd with decoded_length, and adding a session context_reason for persistent
manual-boundary notices. Existing raw bytes, IDs, and item/event links remain
unchanged; migration does not compress old payloads. Schema 5 upgrades to 6 by adding nullable model_calls.retry_of with a self-reference;
existing calls stay unchanged. Schema 6 upgrades to 7 with persona_states, model_sessions.response_schema, and
model_calls.state_before/validation_errors. Existing sessions keep a NULL contract
and remain active until an explicit fresh/connection boundary; their bytes are
never reinterpreted. See [state and upgrade rules](state.md). Schema 8 adds real
group memberships/mentions and persona-wide call identity/order, preserving all
IDs, records, frozen bytes, associations, and editable files. A database
from a newer app is rejected rather than downgraded. The database uses WAL,
foreign keys, and short transactions; generation waits do not hold SQL locks.

## Validation

Rust integration tests cover migration, exact request replay after later
turns, session reuse, frozen prefix bytes after a file edit, own-output
association, persona isolation, cancellation, rollback during acceptance and
publication, exclusive ownership, and abrupt process exit followed by restart.
Mock HTTP/SSE tests also cover authentication, two-turn native replay after
restart, UTF-8 fragmentation, content/reasoning isolation, early and idle-stream
cancellation, failure publication, and changed connection boundaries. The Swift
bridge test covers typed records/errors, durable identity, Unicode
streaming, early cancellation, callback release, subscription closure, and a
fresh presentation model restoring and continuing history. Native render
checks exercise the real composer, including marked text and keyboard events.

## Group acceptance and publication

Group acceptance commits a single user message, its publication ordinal, all
current-member received events, resolved mention IDs/intervals, and selected
reply placeholders/calls in one transaction. Silent members have events without
calls. An accepted character reply assigns its publication ordinal and fans out
only `messages.content` to other members in the same transaction as its native
output/state/sent event. Its author has one sent event and native links, never
a second received copy. Failure/cancel/interruption creates no publication,
state transition, accepted native output, or reply fanout.

Membership intervals compare publication order, not display sequence assigned
when a placeholder queued. Existing v0.1.2 complete private messages are assigned
their old display sequence as publication order; unpublished attempts remain
NULL. Migration retains every old conversation/message/session/item/event ID,
byte payload, state, and association. Calls receive their canonical private
persona and old reply display sequence as the initial persona-wide call order.
Future calls allocate from that persona's maximum, across all rooms. Schema 8
adds memberships and mentions to the prior nine tables and verifies foreign keys
before commit.

Schema 10 adds `group_rounds`, `group_reply_slots`, optional `model_calls.slot_id`
and decision generation/phase/knowledge versions. Populated schema 9 upgrades
without rewriting calls, native bytes/links, decisions, state or character files.
There are fourteen application tables; the inventory above is authoritative.
Unfinished rounds/slots become interrupted on startup; no observer resumes them.
See [room flow](group-flow.md) for admission/publication linearization.
