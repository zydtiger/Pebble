# Persona memory and explicit consolidation

Memory belongs to one character across private and group rooms. New supported
contexts expose the native memory tool `update_memory`, whose sole argument `content`
is the complete revised MEMORY.md. Rust chooses the persona directory and file;
this tool cannot choose paths or perform work. External grounding and work use
the separate [delegation contract](delegation.md). The model should
preserve useful prior memory, correct outdated facts, distinguish fictional and
real context, and avoid copying current affinity/mood state into long-term memory.
A write is optional in ordinary conversation. Unchanged content succeeds without
rewriting the file. Claims of saving require a successful native tool result.

The frozen prefix is immutable. Every new memory-capable chat input carries a
current MEMORY snapshot at its tail, including manual edits before activation.
The journal uses that same snapshot as expected-before. A changed file after the
snapshot fails explicitly. A pre-publication conflict leaves the manual target
unchanged; a save racing publication is retained in the operation's displaced file
as described below. Existing authored files
are never shortened by migration or load; only new tool replacements have the
8192 UTF-8 byte cap. Complete argument JSON is bounded to 49166 bytes, allowing
worst-case escaping (`8192 * 6 + 14`, including the object envelope). Unknown names, extra fields, malformed JSON, missing IDs,
multiple calls or oversized content fail before any effect.

## Durable effects and native replay

One persona queue owns chat and maintenance effects across all rooms. A completed
native assistant call batch is archived, and a prepared journal operation records
its actual call ID/arguments plus old/new memory snapshots before the file effect.
Changed content is staged and synced in the operation-owned sibling
`.MEMORY-<operation-id>.displaced`. An atomic exchange (macOS `RENAME_SWAP`, Linux
`RENAME_EXCHANGE`) publishes the proposed file while preserving the actual displaced
inode at that sibling path, then syncs the parent directory. Unsupported filesystems
fail explicitly. The retained file is not automatically deleted or exchanged back.
This is not a content compare-and-swap coordinated with external editors: a save
between the last comparison and exchange can leave the proposed bytes at MEMORY.md.
Rust checks the displaced content, reports that conflict and its recovery path,
and refuses the reply/context publication. Users inspect both files and restore
the desired content manually. An editor already holding the old file descriptor
can write after the check; those later bytes remain in the retained inode even
when that late edit cannot be detected immediately. This also explains why
successful changed operations retain their one displaced file. No-change reviews
create no sibling file. There is no generic file-version or automatic cleanup service.
Durable result publication records success/no-change/failure and the native output
in order. A failed memory effect visibly fails the ordinary reply before any final
candidate can conceal it; an explicit retry replays that failure with tools disabled.
Chat's
frozen retry boundary advances through that result. A final conversational reply
still must validate `{reply,state}` before its text/state publishes.

A committed memory change survives a failed/cancelled final reply. The failed
bubble explains that partial outcome; retry replays the recorded assistant call
and tool result with tools disabled. It never repeats that saved write. Restart
reconciles a prepared operation against the target and retained displaced evidence.
A matching proposed target with the expected displaced content is committed; a
staged new file with the unchanged old target is interrupted before publication.
Displaced conflict takes precedence over a proposed target, so it cannot become a
successful review on restart. Changed publication requires readable expected
displaced evidence: a proposed target alone is never proof that Pebble performed
the exchange. Only identical no-change review can succeed without a sidecar.
Missing/unreadable/incomplete evidence fails visibly and preserves the target. Recovery never automatically performs a missing file write. A duplicate
provider ID with identical arguments returns the saved result; differing arguments
fail. If the file was saved but result bookkeeping failed, the partial outcome is reported
immediately and that persona is blocked until restart recovery.
Snapshots/results remain durable in SQLite, independently of final reply/state.

One chat permits at most one native memory call and four HTTP requests: the auto
tool/direct-final phase plus at most three final candidates after a tool call.
All share a 180-second operation deadline and the configured per-request output
cap, so the total requested output is at most four times that cap. Memory-capable
sessions also bound each response's decoded model output to 128 KiB, including
reply/refusal text, reasoning, opaque native fields and tool arguments. SSE
framing and repeated metadata do not consume this output allowance; Responses
delta, done and final representations of the same item are counted once.
Individual SSE events/lines retain their independent 1 MiB limit. The MEMORY and
tool-argument limits above remain independent. Preserved pre-upgrade sessions
retain their earlier native-stream behavior. Final/repair requests disable tools and apply the selected
schema dialect. A rejected candidate is not accepted native output.

## Start fresh context

The sparkle action is an asynchronous same-persona maintenance operation. It
requires that persona to be idle at acceptance, including pending group slots and
participation. It freezes the event cutoff and unchanged accepted state, loads
all active native knowledge and eligible unconsumed observations, and asks the
same CHAT connection to review them with current MEMORY. Other personas' private
input is never included. Silent public group observations count even without a
prior model session. The model must call update_memory once, including a complete
unchanged document for a no-change review. Maintenance accepts no conversational
reply or state change.

After success Rust atomically retires the old session and saves current SOUL,
updated MEMORY, the same accepted state, and one marker. New observations beyond
the frozen cutoff remain consumable. Native archives and visible transcripts
remain intact. Empty outgoing knowledge needs no model call. Demo transitions
keep MEMORY unchanged and explicitly explain that no chat model is connected.
Maintenance freezes and revalidates the credential-free protocol/model/API-root
binding before publication, including first-session reviews. Budget and credential
edits do not change this identity. Changed provider bindings use their separate
existing chat boundary; maintenance
refuses to send old native history to the changed endpoint.

Cancellation before effects preserves memory and context. Cancellation after a
committed memory write keeps that write, preserves old context and reports the
partial result. A durably completed identical review has `memory_reviewed=true`
and `memory_saved=false`; its failure/cancellation text explains that review is
complete while the old context remains active. Both flags and explanation survive
restart. Context-publication failure has the same completed-review boundary.
Explicit retry recovers the same operation/cutoff and publishes its saved result
without another model call or write while its native-history snapshot, state and
current memory remain unchanged. Intervening native history/state or manual memory
edits supersede the unfinished boundary; the next explicit action reviews current
knowledge and MEMORY anew, retaining the earlier committed effect in its archive.
Unconsumed newer observations alone do not invalidate boundary recovery.
Restart exposes interrupted/partial status
through typed FFI snapshots and native recovery text. A successful context has one
marker; failed/cancelled operations have none. Corrupt, unreadable or oversized
sources fail rather than silently empty-resetting or truncating knowledge.

## Visible boundaries

Rust projects a successful persona marker into its private room and groups with
an active membership interval at publication. Removal during maintenance excludes
that group; later joins do not backfill old markers. The tooltip and accessibility label identify
only the affected character. Each projection stores its current last allocated
message sequence as a display anchor. This is independent from the frozen persona
event cutoff and membership publication order. Other speakers' pending placeholders
may appear before the line and complete later; the marker never implies a group
reset or that those later public observations were summarized. Swift renders a
thin gray horizontal line outside bubbles, with attribution in its tooltip and
accessibility label.
The marker is never a model input or received/sent event.

See [context](context.md#maintenance-headroom) for conservative reserve arithmetic,
[providers](providers.md#native-memory-tools) for protocol contracts, and
[history](history.md) for schema 11 storage. This memory subsystem includes no threshold rollover or retrieval.
[Images](images.md) and [delegation](delegation.md) have separate contracts.
