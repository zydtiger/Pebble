# Native conversation recall

Characters can use Rust-owned read-only tools to recall Pebble conversations they
received. This capability is separate from Codex delegation and needs no external
service, embeddings or vector database. New supported contexts expose these tools;
preserved contexts keep their frozen capability until a supported fresh boundary.

## Search and read contracts

`search_history` accepts `content`, `date_from`, `date_to`, `conversation_id`,
`limit`, and `cursor`. Nullable fields use JSON null when omitted by the model.
At least one nonempty content or date filter is required. Content is a literal,
case-sensitive substring of at most 1024 UTF-8 bytes, supporting Chinese and
English. SQL parameters preserve quote, percent and underscore characters as
literal text. The optional room ID narrows the same visibility scope. `limit`
is 1–20; a nonnegative event-sequence cursor permits forward pagination within
the same frozen reply snapshot. Each result includes a stable message ID, room,
speaker, UTC creation timestamp, bounded text snippet, truncation flag and cursor.
Results report whether another page exists.

Dates must be real `YYYY-MM-DD` calendar dates from 1970–9999. `date_from` and
`date_to` include their local days in the host's configured time zone. Rust derives
local midnight boundaries for each date using the host calendar/zone, including
23-hour and 25-hour daylight-saving days; it does not assume a day is 86400 seconds
or reinterpret dates as UTC. Reversed ranges and invalid dates fail explicitly.
One-sided date filters are supported. Source timestamps remain UTC for provenance.

`read_history` accepts a visible `message_id` and `before`/`after` counts from
0–4. It returns the anchor and a bounded visible neighborhood in that room's
transcript order. It never fills visibility gaps with unseen messages. Search
snippets use up to 384 UTF-8 bytes; read excerpts use up to 1536 bytes per message,
with safe Unicode boundaries. Serialized message evidence has a 24 KiB cap,
including JSON escaping; shortened excerpts carry truncation flags. No image
payload, reasoning, system prefix, native provider internals or work logs is read.

## Visibility, evidence and replay

A persona-visible received or sent `persona_events` record is required. The tools
can read that character's private chat and historically received group messages,
even after leaving a group. Current membership never grants prior/backfill access,
and another character's private chat is excluded. Only completed visible messages
are eligible. At activation Rust freezes the maximum existing persona event
sequence, including already accepted predecessor output. Input arriving later
cannot enter lookup evidence. Retry retains that snapshot and saved tool results.

Historical participant text is untrusted evidence, not instructions. Tool results
and the frozen persona prefix state this explicitly. Private information is not
automatically suitable for disclosure in a public room; the model must preserve
that boundary while composing its public reply.

The native loop permits at most four history calls followed by one optional
existing memory, image or delegation effect and a final structured response.
Read-only results count toward the complete request budget. Calls/results are
checkpointed as balanced native exchanges before another request, and survive
failure/restart without silently re-querying newer history. MEMORY replacement
remains exactly 8192 UTF-8 bytes; history lookup does not change or bypass that cap.
