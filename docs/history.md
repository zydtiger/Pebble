# Durable private-chat history

v0.0.3 uses one SQLite database and a live Chat Completions adapter. An
unconfigured character uses a deterministic local demo. There is no group
scheduler, classifier, memory writer, or work-agent integration.

## Records and ordering

| Table | Identity, order, and responsibility |
| --- | --- |
| `personas` | Stable UUID and directory; display name is derived from files |
| `conversations` | One canonical `direct` room per persona; existing IDs survive migration |
| `messages` | Visible content, explicit user/persona sender, status, timestamp; unique `(conversation_id, sequence)` |
| `persona_events` | Received user messages and accepted sent replies; unique `(persona_id, sequence)` and `(persona_id, message_id)` |
| `model_sessions` | One active context per persona; protocol/model/API-root binding, event start boundary, and covered-event frontier |
| `model_items` | Frozen prefix/input/output JSON BLOBs; unique `(session_id, sequence)`; optional producing call |
| `model_item_events` | Explicit item/event links, including own outputs; indexes support reverse lookup |
| `model_calls` | Session, reply room, trigger/reply message IDs, input item cutoff, event cutoff, status and error |

All sequences are assigned inside transactions. Timestamps describe creation;
they do not reorder stored history. A message ID identifies the same bubble
while streaming and after restart. The runtime owns the reply destination.
One session belongs to a persona, not an app launch or visible room.

The first input initializes a session with a fixed application/SOUL/MEMORY
snapshot. Reloading files changes the character panel, but does not rewrite
that existing snapshot. Sessions identify `pebble_demo_v1` / `local-demo` or
`chat_completions` / the configured model. The credential-free `base_url` is
stored on the session; keys are resolved at runtime and never persisted.

Changing the API root, model, or protocol retires the previous session. The new
session's `event_start_sequence` is the prior persona-event maximum, so its
request includes only later events. This fresh context does not import old demo
or provider output. Old visible messages and frozen items remain preserved. A
notice is derived from the stored boundary and is visible after restart too.
General user-triggered resets remain a later milestone.

## One turn

```text
send(persona, text)
  transaction
    user message U1 + streaming reply placeholder A1
    received event E1
    session S (resolve or create fixed prefix P)
    call C1 (reply room, trigger U1, event cutoff, input sequence cutoff)
    frozen input I1 + link I1 -> E1
  commit
  load C1's items from S ordered by sequence: P, I1
  live adapter (or demo) reads that stored request outside the transaction
  checkpoint each text delta before notifying the UI
  completion transaction
    finalize A1 + native assistant output O1
    sent event E2 + link O1 -> E2
    finalize C1 and advance contiguous event coverage
  commit, then notify completion

next turn's request: P, I1, O1, I2
```

The input item is JSON with `role: user` and `content` containing a serialized
envelope. Its `events` include source event/message/room IDs, event sequence,
speaker kind/ID, and text. `reply_request` contains the target room and trigger
message. These are fields inside content, not imaginary provider API fields.
The prefix defines this attribution. Participant text is serialized as data.
The completed output is `role: assistant` plus content and supported native
`reasoning_content`/`refusal` fields; it is stored once. Only content is rendered.

The builder appends events without a representation in the active session.
Its own sent event already links to the original output, so it never becomes
an extra incoming user message. The covered frontier stops before the first
unrepresented event instead of jumping to the largest linked sequence.

A call records the last input item sequence. Reconstructing that call loads
items through this cutoff, retaining the exact saved bytes. It excludes its
own future output and later turns. There is no full-request copy per call,
transcript re-rendering, dynamic timestamp in the prefix, or server KV cache
management. v0.0.3 stores raw JSON; compression and configurable context limits come later.

## Failure and lifetime boundaries

Acceptance and final publication are separate atomic transactions. A failure
cannot publish half an output/event association. Delta checkpoints retain
visible partial text; only successful completion creates an assistant model
item and a sent event. Stopped/failed/interrupted bubbles are explicit in the
UI and do not masquerade as accepted assistant context. The accepted user
input stays in the session even when its reply is cancelled.

One worker runs at a time in this milestone. Rust owns cancellation and durable
terminal states; Swift consumes typed message snapshots and output deltas.
The draft is cleared after Rust accepts it, and callback IDs prevent updating
the wrong visible message. Character switching is disabled while replying;
independent persona queues and background room routing belong to v0.0.4.
Dropping a handle or disconnecting its observer cancels the worker. The UI
reloads the final transcript from Rust. No retry or resubmission is automatic.

An advisory lock on `data/app.lock` reserves the data root for the core and any
worker still using its database. The file contains no chat state; OS ownership
is released on normal or abrupt exit. A second opener fails without touching
running calls. After acquiring ownership, startup transactionally marks any
remaining streaming messages/calls interrupted, preserving their last durable
text and accepted input. It never invents a completed model output. This is
basic restart consistency; retry/reconciliation workflows remain v0.0.7.

Schema version 1 adds the six history tables and indexes; version 2 adds
credential-free API-root binding and an event-start boundary, reaching schema 3
in one transaction. Existing IDs, records, frozen bytes, and editable files
remain intact. A database
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
