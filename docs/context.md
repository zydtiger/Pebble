# Context and native item encoding

Context limits belong to Rust. `[defaults.context]` in app config provides
`context_window_tokens` (65536 by default) and `max_output_tokens` (4096).
A persona's `[context]` overrides each supplied field independently. Both must
be positive, with output strictly below the context window. Queued calls retain
the budget loaded when accepted; editing budgets does not retire sessions or
rewrite frozen native items. Output reserve is sent to the selected adapter.

## Estimate and failure behavior

The conservative input estimate is the total UTF-8 byte length of decoded native
JSON items plus 64 per item. Each byte counts as one estimated token. The output
reserve must fit within the configured window after that estimate. This includes
the fixed prefix, system/SOUL/MEMORY, attributed user envelopes, complete native
assistant output, reasoning, and opaque fields. It deliberately counts JSON and
opaque strings rather than silently excluding state it cannot inspect. Structured schemas and current-state envelopes are part of that input; transient validation feedback also counts when repairing a candidate.

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

## Explicit boundaries

Start fresh context keeps all saved bubbles and native items. Rust serializes
the operation against queue acceptance, refuses queued/active work for that
persona (including pending decisions and still-admittable group slots), and atomically retires its current session and saves a new frozen prefix
with current files and the unchanged accepted persona state. An incompatible response schema fails without replacing the active session or resetting state. The current event frontier becomes the boundary. Subsequent
requests contain only events after it; earlier native output never becomes user
input. A manual reason persists the notice even before the first received event.
Other personas may continue generating.

The UI explains and confirms this nondestructive operation. It does not clear
the transcript, compact/summarize model history, or archive data. Old frozen calls
remain replayable within the same decoding bound. A fresh context can recover
from a full or unreadable old context without loading its payloads.
