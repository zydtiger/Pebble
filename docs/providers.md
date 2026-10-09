# Provider contracts

Both live protocols use an OpenAI-compatible API root, optional runtime
credentials, bounded cancellable HTTP, and byte-framed SSE. An absent connection
selects the deterministic demo; a configured connection never falls back to it.

Chat Completions sends the saved native message list to `/chat/completions`.
Only content deltas are visible. A stop finish and `[DONE]` are required; the
accepted assistant message retains supported reasoning and refusal fields.

Responses sends the saved native input list to `/responses`, with `stream: true`,
`store: false`, `include: ["reasoning.encrypted_content"]`, and
`max_output_tokens: 4096`. It does not send `previous_response_id`. Each accepted
reasoning/message output item becomes one immutable history item. Replay retains
the complete native object, including IDs, status, encrypted content, and
unknown fields. No tool calls are executed or accepted in this milestone.

The [Responses streaming reference](https://developers.openai.com/api/reference/resources/responses/streaming-events)
distinguishes added items from completed items. Added reasoning may be
incomplete; final item events and the completed response output are authoritative.
A successful terminal response must have completed status, nonempty visible
text/refusal, and agreement with streamed text and final item events. Failed or
incomplete responses and truncated streams publish no native output. Reasoning
is neither rendered nor copied into diagnostics.

The decoder accepts explicit item/content indices. Compatible servers that omit
indices can identify items by ID and content parts by their added/done lifecycle.
An omitted content index is resolved only while that item has one active content
part; missing or overlapping lifecycle state fails rather than guessing an index.
Final output array order controls persistence even if item-done events arrive in
a different order.

Ordinary tests use deterministic local servers. The opt-in tests documented in
README use synthetic temporary characters and verify actual native replay across
two turns and restart. They never read the app's existing files or conversation.
