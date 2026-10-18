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
[development](development.md#validate) use synthetic temporary characters and verify actual native replay across
two turns and restart. They never read the app's existing files or conversation.

## Structured-output constraints

By default, final-only structured requests send Chat Completions `response_format` with
`type: "json_schema"` and a strict named `json_schema`. Standard Responses sends
`text.format` with `type: "json_schema"`, name, strict, and the schema. The frozen
prefix also includes the schema and response semantics; provider constraints do
not replace Rust validation. Legacy sessions send neither constraint.

DeepSeek Chat Completions at the exact hostname `api.deepseek.com` instead sends
`response_format: {"type":"json_object"}` for both final chat replies and every
participation phase. Both the API root and `/v1` root select this behavior.
DeepSeek's Chat Completions endpoint rejects `json_schema`; JSON mode constrains
JSON syntax only. The frozen chat prefix still carries the response schema;
participation prompts still specify the required boolean. Rust validates the same
reply/state schema and strict participation fields before accepting either output.
Chat retains its bounded format-repair behavior; classifier failures are not retried.
Tool-enabled chat requests retain their native tool declarations and final-only
format selection. No API key, history, schema, or settings migration is needed.

This rule is specific to the API host and Chat Completions protocol, not the model
name or whether a server is hosted. Local SGLang/vLLM/llama.cpp, OpenAI, and other
hosts retain the existing schema mode. Proxies under another hostname do not
inherit the DeepSeek rule. Responses behavior is unchanged. There is no HTTP-error
fallback or capability probe that silently weakens a schema constraint.

On 2026-10-17, `deepseek-flash` rejected both chat and classifier `json_schema`
requests with HTTP 400, while JSON mode passed. Isolated live tests through
Pebble then verified validated chat/state acceptance, a second turn after restart,
and true/false participation decisions. Credentials are supplied through
`PEBBLE_DEEPSEEK_CHAT_KEY` and `PEBBLE_DEEPSEEK_CLASSIFIER_KEY`; the tests never
read the application's files or save keys into test configuration:

```sh
cargo test -p pebble-core --test deepseek_live -- --ignored
cargo test -p pebble-core --lib deepseek_participation_accepts_true_and_false -- --ignored
```

See [DeepSeek JSON Output](https://api-docs.deepseek.com/guides/json_mode/) for
JSON mode's prompt requirements and limitations. JSON syntax guarantees are
weaker than schema adherence, so local validation remains mandatory in both modes.

For structured JSON-mode requests, Pebble also includes the actual current-purpose
schema as derived system context. Private no-reply evaluations use a different
shape from the frozen chat prefix; Rust validates their state, plans and exact
outcome acknowledgments before accepting the handover. Memory maintenance stays
tool-only.

The opt-in `activity_persona_live` test uses a selected real persona endpoint
with an isolated synthetic character and a gated local task executor. It verifies
native delegation, chat while the accepted task continues, exact private outcome
acceptance and absence of private chat bubbles. It does not execute real Codex
research; `activity_live` separately covers the real Codex executor.


Some compatible Responses servers expose the Chat Completions constraint as an
extension instead of implementing standard `text.format`. Explicitly configure
that connection with `responses_schema_format = "response_format"`; default or
`"text_format"` selects standard Responses. This field is valid only for
`api = "responses"`; invalid/irrelevant selections fail configuration validation.
It can be set in app-default or persona connection TOML, or the native Responses
form's **Response schema** picker: **Standard · text.format** or
**llama.cpp · response_format**. Native settings saves for Responses preserve
the selection; switching the connection to Chat Completions removes
this Responses-only field. No automatic Responses capability fallback occurs.

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
explicit Qwen `chat_template_kwargs.enable_thinking = false` for separate participation.
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

New sessions expose `update_memory({content: complete revised MEMORY.md})` and
the separate [delegate_task contract](delegation.md). One bounded native tool
effect is permitted per conversational reply; a replayed/retried effect disables
further effects for that reply.
Ordinary chat starts with `tool_choice=auto`, `parallel_tool_calls=false` and no
provider response-format constraint; Rust still validates a direct final
`{reply,state}` locally. The selected compatible service suppresses native tools
when Chat Completions auto tools and response_format are combined. A tool result
is followed by a final-only request with tools disabled and the configured strict
schema dialect; schema repair also disables tools. Maintenance exposes only `update_memory`, even when outgoing history contains
delegation calls. It requires one native call (`tool_choice=required`) and has no conversational final request.

Chat Completions assembles indexed fragmented IDs/names/argument strings and
retains assistant reasoning with the native call. Null tool ID, type, name or
argument fields mean no update for that chunk; previously assembled values are
retained. Non-null invalid types and incomplete final tool identities still fail. Responses uses authoritative
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

## Request parameters and thinking support

Concrete [request settings](settings.md#concrete-request-parameters) are composed
before final budgeting for both protocols, including repair and memory phases.
Pebble reserves native fields and rejects alternate output caps such as llama.cpp
`n_predict`, schema/grammar and reasoning-parser overrides. No arbitrary merge
can silently replace those contracts.

On 2026-10-10, bounded synthetic math probes against the selected
`gemma-4-31b-uncensored` llama-server build `b11146-7fe450e19` and the independent
`Qwen/Qwen3.5-2B` service measured nested `enable_thinking=false` producing the
correct answer without reasoning. The same prompt with `true` produced reasoning;
Qwen exhausted the probe's 256-token cap before visible content. These observations
establish a behavior change on those two deployments, not universal parameter
support or a guarantee that the application's full structured contract succeeds.
The [llama.cpp server contract](https://github.com/ggml-org/llama.cpp/blob/7fe450e19/tools/server/README.md)
and [Qwen model card](https://huggingface.co/Qwen/Qwen3.5-2B)
describe their concrete template controls. Sampling and this per-request switch
preserve Pebble history; other template options require a compatible fresh boundary.

## Image input

Configured chat and classifier endpoints are assumed to accept vision input.
There is no capability toggle, model-name inference or text-only fallback.
Chat Completions uses attributed text plus native `image_url` content parts;
Responses uses `input_text` and `input_image`. Data URLs contain verified saved
renditions, never external filesystem paths. All classifier phases and explicit
memory consolidation use the same adapter boundary. Endpoint rejection is a
visible failure. [Image input](images.md) records the immutable-reference format,
768-pixel limit, separate vision reserve, selected-service measurements and
synthetic visual-control checks.

## Native history tools

New contexts declare Rust-owned `search_history` and `read_history` in both API
formats. Each operation admits one native call at a time, with at most four
read-only lookups followed by one optional existing memory/delegation/image effect
and the final structured response. Lookup allowance belongs to the current call
and its retry ancestors, not earlier successful turns. The dispatcher validates
and routes history calls before effect-specific parsers. Results and source
calls persist in native order before continuing; retries replay frozen evidence.
Readonly results count in the request budget and never stuff the full transcript
into the prefix. Final/repair requests disable tools and retain declarations needed
for native replay. Maintenance exposes only update_memory, including when reviewing
history lookup evidence. Preserved contexts retain their capability until an
supported fresh boundary. See [tool contracts](history-tools.md).
