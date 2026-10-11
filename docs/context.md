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

The conservative input estimate is the UTF-8 byte length of the final wire JSON
plus a 128-byte framing reserve, with one byte counted as one estimated token.
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
must satisfy `wire_json_bytes + 128 <= context_len - max_output_tokens - reserve`.
For 262144/4096, reserve is 87381 and the initial ceiling is 170667 bytes.
Tool continuation and final repair use the full `context_len - max_output_tokens`
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
