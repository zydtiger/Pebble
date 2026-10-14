# Context and native item encoding

Context limits belong to each connection in Rust. App `[defaults.chat]` and
`[defaults.classifier]`, and persona `[chat]` and `[classifier]`, require
`context_len` and `max_output_tokens` alongside model, API root and protocol.
`context_len` is the total input-plus-output token window. Both limits must be
positive, with output strictly below the window. An omitted persona connection
inherits the whole respective app connection; an explicit override supplies its
own limits. Missing/malformed limits and obsolete separate context tables fail
with actionable configuration errors. Only the unconfigured demo has an internal
65536/4096 budget. The seeded classifier uses 16384/64.

Accepted work retains its effective budget. Budget-only edits do not retire
sessions or rewrite frozen native items, prefix or accepted state. The selected
output cap is sent to either protocol, including structured repair attempts.
`context_len` is never sent as a provider API parameter.

## Estimate and failure behavior

The conservative input estimate is the UTF-8 byte length of the final JSON's
text and metadata plus a 128-byte framing reserve, with one byte counted as one
estimated token. Image payloads are replaced in this estimate by fixed metadata
and a nonzero 2048-token reserve per image occurrence; base64 is never counted
as ordinary text. The final transport has a separate 32 MiB hard limit. See
[image limits and selected-service evidence](images.md).
The output cap must fit after that estimate. This covers the fixed prefix,
system/SOUL/MEMORY, attributed user envelopes, complete native assistant output,
reasoning, opaque state, current-state envelopes, response schema, protocol/profile
metadata and any tool metadata present. Structured repair feedback counts too.

Participation checks the same complete request accounting using its own
connection's limits on initial decisions, reassessment and both fallback passes.
Optional older ordinary participation excerpts may be removed, preserving
chronological order, actual trigger identity and all eligible same-round
predecessors. Required root/trigger/predecessor messages are selected before
applying the optional eight-event excerpt limits and retain their full text.
Permission retains all supplied real-user transcript/silence inputs to avoid
silently dropping an active restriction; intent retains its complete latest
canonical user message. Mandatory content that cannot fit fails visibly before
HTTP, never becomes false, and never consumes chat coverage/state/history.

This is not exact provider token accounting. Tokenizer behavior, native message
framing, encrypted state, and provider-side costs are uncertain. The estimate
may reject a request that the provider could accept, and does not guarantee the
provider will accept every request. There is no tokenizer service or automatic
endpoint introspection. Context-full fails before credentials/HTTP are used;
the accepted input stays in model history and its visible reply becomes failed.
The user can increase configured limits or deliberately start a fresh context.

## Storage and memory bounds

The [history inventory](history.md) defines current tables, including persona-wide
accepted state, group memberships, decisions and durable reply slots. `model_items.codec` is `json` or `zstd`, with a
nonnegative `decoded_length`. Migration records the length of existing raw bytes
without rewriting or recompressing them. New items below 1024 bytes remain raw;
larger items use zstd level 3 only when the compressed bytes are smaller.
Decoded bytes, IDs, item order, and event links remain exact across restart.

Before fetching any BLOB, request loading checks every item's codec, stored
size, decoded length, and the total decoded bytes plus 64 bytes per item against
an independent 8 MiB bound. Increasing token budgets cannot raise it. Decoding
limits the zstd window to 8 MiB and reads at most the allowed decoded length plus
one byte, bounded by remaining context capacity independently of metadata.
Length mismatch, unsupported codec, and truncated/corrupt payloads fail with an
actionable error. The bound covers saved decoded context, not total process RSS:
transport, JSON parsing, UI, and database buffers have separate allocations.

Representative synthetic measurements from the integration fixture (repeated
opaque Unicode text, not a user corpus): a Chat Completions native item used
13561 decoded bytes / 94 stored bytes; a Responses reasoning item used
13702 / 169. These highly repetitive samples demonstrate the mechanism, not an
expected production compression ratio. Tests also keep noncompressible bytes raw.

## Maintenance headroom

New live memory-capable sessions reserve `min(98304, context_len / 3)` bytes
(integer division) below the ordinary chat input ceiling. An initial chat request
must satisfy `estimated_input <= context_len - max_output_tokens - reserve`.
For 262144/4096, reserve is 87381 and the initial ceiling is 170667 bytes.
Bounded history-tool continuation and final repair use the full `context_len - max_output_tokens`
ceiling. Existing frozen contexts retain their earlier accounting until an
explicit supported transition. Classifier and demo accounting is unchanged.

This is conservative operational room, not an assertion of provider capacity.
The bounded replacement is 8192 UTF-8 bytes; complete arguments allow 49166 bytes
for worst-case JSON escaping (`8192 * 6 + 14`), with one tool call and a small result envelope.
The reserve also leaves room for current MEMORY, maintenance instructions and
observation envelopes in ordinary cases. It is not a proof that every outgoing
request fits: current authored memory, opaque history and new observations can
be larger. Maintenance checks the exact complete serialized request against the
full ceiling before HTTP. Failure preserves source context and reports the
required estimate; it never truncates observations or starts an empty reset.
Unit tests exercise equality and one-byte-over for both ceilings.

## Explicit boundaries

**Start fresh context** is asynchronous, persona-wide memory maintenance. It
refuses outstanding work at acceptance, freezes a persona event cutoff and
loads all active native knowledge plus eligible unconsumed observations through
that cutoff. Silent group observations count even when the persona never had a
model session. The same configured chat model must complete `update_memory`,
including an explicit no-change review, before Rust atomically publishes the
new current SOUL/MEMORY prefix, unchanged accepted state and one durable marker.
No conversational reply is generated and no character state is accepted.
Later observations remain consumable. Other personas can run concurrently.

A failed/cancelled attempt preserves the active source and adds no marker.
Saved memory is durable independently of boundary publication; retry publishes
that saved result with its original cutoff, without another model/write effect.
Unreadable, corrupt or oversized sources fail explicitly. Old calls/items stay
saved and replayable within the loader bound. Empty outgoing knowledge requires
no HTTP; demo transitions preserve MEMORY with an explicit no-model explanation.
Provider changes and schema/state resets retain their distinct existing
boundaries. Maintenance refuses a changed provider binding before sending any
old native history; a normal chat first establishes that connection's context.
See [memory semantics and durability](memory.md).

## Automatic rollover

Supported live memory-capable structured contexts enable automatic review without
additional settings. Let `C = context_len - max_output_tokens - min(98304,
floor(context_len/3))`. After a successful ordinary, image-result, or delegated-result
reply publishes, Rust estimates the complete outgoing native context. At
`floor(0.8*C)`, it reviews memory when no same-persona queued call, decision or
reply slot remains. At `floor(0.9*C)`, it reviews before the next unstarted call
even with a backlog. A known next request projected at or above the priority
threshold also triggers review. Between thresholds queued work can proceed if
its existing hard request guard permits, then the publication is reevaluated.

The current queue owner holds its reservation through review. New input remains
accepted and visible, awaiting only that persona; other personas and background
tasks keep running. Review never interrupts a native tool sequence. A separate
cancellation control prevents disposal of the completed reply handle from stopping
maintenance; closing the core cancels it through the queue.

Automatic review includes only existing native knowledge, not all receipts up to
the latest assistant event. The successor inherits exact summarized event IDs and
the old start boundary, preserving holes, room identity and queued report routing.
A failed or interrupted attempt is durably suppressed for the unchanged native
source. A fixed prefix already at the soft threshold cannot benefit from rollover
and is left to the existing budget failure. Oversized new input still fails before
HTTP. No recursive rollover or automatic retry of a failed model call occurs.
Manual Start fresh context can recover a committed partial review as described in
[memory](memory.md); transcripts, state and original native items remain saved.

### Recent messages after automatic review

Automatic publication also freezes an ordered list of up to ten complete visible
user/character messages already represented in the outgoing persona context.
The list spans private and group rooms in persona event order. Unseen or queued
receipts are excluded even when their sequence precedes a published reply.
Successive rollovers consider inherited covered messages and newly consumed
messages together, without reinjecting those messages as new events.

`session_recent_messages` stores message IDs, positions, and frozen speaker/room
display metadata. It stores neither message bodies nor native model items.
Requests read the original immutable message text and attachment references and
place a separately attributed `recent_history` envelope after the frozen system
prefix. No reasoning, state snapshot or native tool exchange is carried; old tool
effects are not replayed. Original speaker/message/room IDs and UTC timestamps
remain attached. Names and list membership never drift with later renames or input.

Ten is an upper bound. Admission drops complete oldest messages until the fresh
prefix plus carry fits a conservative target of half the ordinary ceiling `C`,
including tool declarations, image cost and transport bounds. It never truncates a
message to claim it was kept whole. If even the newest complete message cannot
fit, carry is empty. A fixed prefix already above that target is not shrunk;
no messages are added and the existing fixed-prefix/hard-budget guards remain.
This leaves room below the automatic soft threshold for upcoming work.

Carry is reconstructed from the request's saved session, including archived
maintenance recovery and retries after restart; runtime assembled text is not
serialized into an extra native archive. Full outgoing knowledge, including its
previous carry, still goes through the existing 8192-byte MEMORY review.
Explicit Start fresh context continues to publish a clean prefix without carry;
recovering a previously committed automatic review retains that review's original
automatic boundary semantics.
