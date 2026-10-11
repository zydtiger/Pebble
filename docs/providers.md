# Provider contracts

Both live protocols use an OpenAI-compatible API root, optional runtime
credentials, bounded cancellable HTTP, and byte-framed SSE. An absent connection
selects the deterministic demo; a configured connection never falls back to it.

Chat Completions sends the saved native message list to `/chat/completions`.
Content deltas are buffered privately until structured validation and acceptance. A conversational stop finish and `[DONE]` are required; native memory calls use `tool_calls` finish plus `[DONE]`. The
accepted assistant message retains supported reasoning fields. Legacy plaintext
contexts also retain refusal fields when nonempty content is present; structured
contexts reject provider refusal before acceptance.

Responses sends the saved native input list to `/responses`, with `stream: true`,
`store: false`, `include: ["reasoning.encrypted_content"]`, and
the configured `max_output_tokens` (4096 by default). It does not send `previous_response_id`. Each accepted
reasoning/message output item becomes one immutable history item. Replay retains
the complete native object, including IDs, status, encrypted content, and
unknown fields. New memory-capable contexts also accept completed `function_call` items for `update_memory` and replay `function_call_output` with the exact `call_id`. Unknown tools fail explicitly.

The [Responses streaming reference](https://developers.openai.com/api/reference/resources/responses/streaming-events)
distinguishes added items from completed items. Added reasoning may be
incomplete; final item events and the completed response output are authoritative.
A successful terminal response must have completed status, nonempty visible
text (or a separately classified refusal or supported native function call), and agreement with streamed text and final item events. Failed or
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

Final-only structured requests send Chat Completions `response_format` with
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


## Consolidated live coverage

The v0.2.0 isolated integration uses Gemma for structured Chat Completions and
the explicitly configured Qwen non-thinking profile for separate participation.
It records raw synthetic requests/responses, initial primary results separately
from later phases, and bounded whole-round outcomes; see
[measured limits](participation.md#validation-and-measured-limits). It does not
claim live Responses support for Qwen. Both classifier protocols and explicit
schema dialects remain tested with deterministic HTTP fixtures. Earlier separate
Gemma live checks cover Chat Completions and its explicitly selected Responses
`response_format` extension; standard `text.format` support must not be inferred
from successful unconstrained output. Provider/model judgment, context estimates
and schema-keyword support remain limited as documented above.

## Native memory tools

New sessions expose only `update_memory({content: complete revised MEMORY.md})`.
Ordinary chat starts with `tool_choice=auto`, `parallel_tool_calls=false` and no
provider response-format constraint; Rust still validates a direct final
`{reply,state}` locally. The selected compatible service suppresses native tools
when Chat Completions auto tools and response_format are combined. A tool result
is followed by a final-only request with tools disabled and the configured strict
schema dialect; schema repair also disables tools. Maintenance requires one
native call (`tool_choice=required`) and has no conversational final request.

Chat Completions assembles indexed fragmented IDs/names/argument strings and
retains assistant reasoning with the native call. Responses uses authoritative
completed reasoning/function-call items in their array order and preserves opaque
fields/IDs; final item events must agree. Pebble retains completed native tool
batches, arguments and results even when a final reply fails. Tool output and
reasoning never become visible bubbles or public group observations. See
[memory durability](memory.md) for exactly-once effect/retry boundaries.

Both streaming adapters passed the opt-in isolated synthetic memory flow against
the selected Gemma service: native save, validated final reply, explicit
maintenance and continued chat from the new prefix. Run it explicitly with
`PEBBLE_LIVE_API_ROOT` and `PEBBLE_LIVE_MODEL` using the ignored
`opt_in_live_memory_and_maintenance_both_native_protocols` test in
`tests/memory_context.rs`. The Responses branch selects the documented compatible
`response_format` dialect; this does not claim standard text.format enforcement.
