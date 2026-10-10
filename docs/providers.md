# Provider contracts

Both live protocols use an OpenAI-compatible API root, optional runtime
credentials, bounded cancellable HTTP, and byte-framed SSE. An absent connection
selects the deterministic demo; a configured connection never falls back to it.

Chat Completions sends the saved native message list to `/chat/completions`.
Content deltas are buffered privately until structured validation and acceptance. A stop finish and `[DONE]` are required; the
accepted assistant message retains supported reasoning fields. Legacy plaintext
contexts also retain refusal fields when nonempty content is present; structured
contexts reject provider refusal before acceptance.

Responses sends the saved native input list to `/responses`, with `stream: true`,
`store: false`, `include: ["reasoning.encrypted_content"]`, and
the configured `max_output_tokens` (4096 by default). It does not send `previous_response_id`. Each accepted
reasoning/message output item becomes one immutable history item. Replay retains
the complete native object, including IDs, status, encrypted content, and
unknown fields. No tool calls are executed or accepted in this milestone.

The [Responses streaming reference](https://developers.openai.com/api/reference/resources/responses/streaming-events)
distinguishes added items from completed items. Added reasoning may be
incomplete; final item events and the completed response output are authoritative.
A successful terminal response must have completed status, nonempty visible
text (or a separately classified refusal), and agreement with streamed text and final item events. Failed or
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

## Structured-output constraints

New structured contexts send Chat Completions `response_format` with
`type: "json_schema"` and a strict named `json_schema`. Standard Responses sends
`text.format` with `type: "json_schema"`, name, strict, and the schema. The frozen
prefix also includes the schema and response semantics; provider constraints do
not replace Rust validation. Legacy sessions send neither constraint.

Some compatible Responses servers expose the Chat Completions constraint as an
extension instead of implementing standard `text.format`. Explicitly configure
that connection with `responses_schema_format = "response_format"`; default or
`"text_format"` selects standard Responses. This field is valid only for
`api = "responses"`; invalid/irrelevant selections fail configuration validation.
It can be set in app-default or persona connection TOML, or the native Responses
form's **Response schema** picker: **Standard · text.format** or
**llama.cpp · response_format**. Native settings saves for Responses preserve
the selection; switching the connection to Chat Completions removes
this Responses-only field. No endpoint sniffing or automatic capability fallback occurs.

On 2026-10-09, synthetic probes against llama-server build `b11146-7fe450e19`,
model `gemma-4-31b-uncensored`, tested a schema enum deliberately contrary to the
prompt. Chat Completions `response_format` enforced the enum. Responses ignored
standard `text.format` and returned the prompt's conflicting values; its explicit
`response_format` extension enforced the enum. Thus merely receiving HTTP 200
does not establish constraint support. The two actual adapters were additionally
exercised with synthetic characters through acceptance, state persistence,
restart/native replay, cancellation, and explicit retry. These observations do
not establish support for every JSON Schema keyword or another server version.
