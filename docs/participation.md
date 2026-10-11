# Selective participation

Participation decisions use an independent connection on user group posts and
bounded bot followups. A separate
small model returns exactly `{"respond":true}` or `{"respond":false}`. The persona's
chat model writes any requested reply. Private messages never trigger classifiers,
and bot followups remain inside the finite [room round](group-flow.md).

## Configuration and provider contract

The new-root seed selects:

```toml
[defaults.classifier]
model = "Qwen/Qwen3.5-2B"
base_url = "http://beluga:4014/v1"
api = "chat_completions"
api_key = ""
context_len = 16384
max_output_tokens = 64

[defaults.classifier.request]
temperature = 0
[defaults.classifier.request.chat_template_kwargs]
enable_thinking = false
```

Existing authored config is never overwritten or extended silently. An absent
classifier means manual mentions with a clear setup status. A persona `[classifier]`
replaces the entire app classifier connection, independently of `[chat]`; it
requires model, API root, explicit protocol and its own positive `context_len`
and `max_output_tokens`, with output smaller than the total window. Omitted
persona connections inherit both limits with the whole app connection. Credentials are independent and
support empty/no authentication, inline key or environment reference. Native
**Participation model** controls expose app defaults and character overrides.
Disabling a character override inherits app defaults. Saving requires idle
replies, decisions and still-admittable slots in the affected scope; validation precedes atomic TOML
replacement. No chat session or state boundary changes.

Both protocols use streaming false, a configured output cap (64 in the seed),
and required-boolean-only strict JSON schema. The seed explicitly configures
`temperature = 0` and nested `chat_template_kwargs.enable_thinking = false`;
otherwise optional parameters are omitted. Both connections use the concrete
[request settings contract](settings.md#concrete-request-parameters).
Chat sends `response_format.json_schema`. Responses sends `store=false`,
`max_output_tokens`, and standard `text.format`, or the explicitly selected
`responses_schema_format = "response_format"` llama-compatible extension.
There is no provider/hostname/model sniffing or alternate-model retry.
Backend support for thinking controls must be verified independently; parameters
remain explicit across protocol switches, with incompatible values rejected.

Rust independently rejects invalid JSON, absent/nonboolean/duplicate/extra fields,
refusal, empty text, unsupported tools/content, and incomplete/truncated outcomes.
A classifier failure is visible, never silently converted to false or retried
through the chat/larger model. HTTP has a 10-second connect and 30-second total
deadline, disabled redirects and a 1 MiB response limit. Diagnostics omit raw
provider response bodies, credentials and model reasoning.

## Input and decision lifecycle

The exact recall-first prompt is [participation-prompt.txt](../crates/pebble-core/assets/participation-prompt.txt).
It permits relevant social, emotional, shared-memory and roleplay participation,
with uncertainty favoring plausible participation while respecting explicit space
or silence. It treats supplied content as data. The compact JSON input includes:

- Target persona ID/name, SOUL (3,000 UTF-8 bytes), MEMORY (2,000 bytes), and
  accepted state (4,096 bytes, or an explicitly labeled truncated excerpt).
- Up to eight optional older complete messages delivered to this persona in the
  target group, and eight optional eligible events from other rooms. Each text excerpt
  is bounded at 500 bytes, with source event/message/room/speaker identity.
  All eligible phase-required root/trigger messages and completed same-round
  predecessors are included independently of those excerpt bounds, with full
  text and the existing delivered-event access/cutoff rules. Later accepted
  events cannot displace an earlier required predecessor.
- Chronological excerpts including accepted same-round predecessors and actual
  already-represented persona knowledge. No retrieval
  or memory consolidation is implied.

Each pass checks the final request JSON (UTF-8 bytes plus 128-byte reserve),
including rules, identity, schema, concrete request parameters and its output cap.
Ordinary decisions/reassessments may drop optional older excerpts to fit;
mandatory trigger and same-round inputs remain. Intent and permission never
trim their mandatory user/permission/silence inputs. Overflow fails visibly
before HTTP, with no false decision or coverage/state/history consumption.
`context_len` is local and never sent as a provider parameter.

Other personas' private events/state/native output/reasoning are never supplied.
A persona may read its own private context; its generated public reply can still
disclose what it knows, so prompt instructions are not a secrecy guarantee.

One canonical user message and all current-member observations commit before
inference. Stable user mentions bypass inference and still permit unmentioned
members to volunteer. Classifiers run concurrently outside persona chat queues.
Their read-only views do not link/consume chat events, append native items,
advance coverage or change state. False becomes **Listening** with no assistant
bubble. True makes a slot eligible; only the next ready room slot creates a call
with immutable destination, trigger, member interval and original receipt cutoff
and joins the normal persona queue.
Generation uses actual accepted prefix/state at activation; newer accepted
cross-room knowledge stays available, without widening selection of new events.

SQLite schema 9 adds `participation_decisions` to populated schema 8 without
rewriting existing records. It stores IDs/cutoff/revision/status/boolean, bounded
diagnostic, elapsed time and admitted call link, not copied classifier inputs.
Schema 10 adds rounds/slots and decision generations; each admission remains unique.
Optional decisions can be re-evaluated when accepted predecessors or current
persona knowledge/state change, including while an admitted call waits behind a
private predecessor. Generation checks reject late classifier callbacks. New
user posts cancel old automatic decisions/calls while protecting outstanding
required mentions. Stop checking, room Stop, listener disconnect and closing
cancel work while preserving public observations. Restart interrupts unfinished
work and never resends. Native observers remain open across waiting slots,
followups, fallback phases and every dynamically admitted speaker. Silent and
failed checks stay outside replayable chat history.

Whole-group fallback uses two separate strict passes only after all initial
decisions are valid false with no required or accepted reply. The exact
[room intent prompt](../crates/pebble-core/assets/fallback-intent-prompt.txt) returns
`{"whole_group_question": boolean}` using only the latest canonical user message
(complete text), without SOUL/interests. Its connection comes from the
first configured eligible candidate in stable order. If true, the exact
[permission prompt](../crates/pebble-core/assets/fallback-permission-prompt.txt)
uses each candidate's own connection, identity and bounded chronological delivered
user transcript, without SOUL/MEMORY/state or interest judgments. It checks silence
and newer permission and returns `{"respond": boolean}`. At most one permitted
candidate acknowledges; completion/failure ends the round without bot followups.
Errors end fallback visibly, never becoming false or triggering another model.
The transcript is bounded, so older restrictions outside its excerpts are not
a durable silence-policy store. Model interpretation remains fallible.

## Validation and measured limits

Deterministic Rust checks cover both protocols/dialects, strict parsing,
privacy/nonconsumption, settings inheritance/credentials/protocol switches,
mention plus volunteer, cancellation/stale callback/restart, populated migration,
and actual newer cross-room prefix/state inheritance. Native bridge/render checks
exercise real Deciding/Listening/error/Stop/manual recovery and dynamic replies.
Manual synthetic fixtures disable classifiers explicitly and never contact the
seed service.

An opt-in integration check uses only synthetic temporary roots and the frozen
unchanged-label subset in `tests/fixtures/participation-regressions.json`:

```sh
mkdir -p build/participation-live
PEBBLE_CLASSIFIER_REPORT="$PWD/build/participation-live/report.json" \
PEBBLE_CLASSIFIER_API_ROOT=http://beluga:4014/v1 \
PEBBLE_CLASSIFIER_MODEL=Qwen/Qwen3.5-2B \
PEBBLE_LIVE_API_ROOT=http://beluga:4013/v1 \
PEBBLE_LIVE_MODEL=gemma-4-31b-uncensored \
cargo test -p pebble-core --test participation_live_endpoint -- --ignored --nocapture
```

`PEBBLE_CLASSIFIER_REPORT` is required and must be an absolute path in an existing
directory. If proxy routing fails, retry only that command with
`NO_PROXY=beluga,127.0.0.1,localhost`.

The final v0.2.0 selected-service run kept the eight frozen labels and scored
TP 5 / FN 0 / FP 0 / TN 3, with no invalid initial response. Metrics use each
case's first raw primary-prompt response, independently of later fallback or
reassessment that can reuse its durable decision row. With two concurrent private
Gemma calls, the four initial group primary responses were all received after
78 ms; the full room round settled after 43,727 ms and completed six group calls
(four initial plus two followups), within N+2. Both private calls completed.
All 25 Qwen requests (including later primary/fallback phases) were schema-valid
with no reasoning content or nonzero reasoning-token count. Eight Gemma requests
completed, with no format-repair request or generation error in this run.

The harness preserves raw requests/responses and separately reports initial
primary response latency, complete round settlement, final lifecycle and admission
bounds. Its synthetic relay buffers complete chat SSE for raw capture; total
round timing includes that relay and full generation. This is an integration
regression using compact/attributed inputs and unchanged labels, not an
independent generalization estimate or a latency guarantee. This final integrated
run used Chat Completions for both models. Responses classification is covered by
local protocol fixtures, not a selected Qwen live capability claim; earlier
separate Gemma checks exercised both chat protocols. The report directory is
validated before service requests begin.

The earlier combined intent-and-permission prompt passed 7/10 frozen development
examples. Splitting the passes scored 9/10 (TP 3 / FN 1 / FP 0 / TN 6), with all
15 requests schema-correct and zero reasoning. The remaining miss applied another
member's silence restriction to an allowed target. These are development probes,
not an independent accuracy estimate or a universal permission guarantee. The
runtime uses the exact split prompts and deterministic routing; integration tests
verify schemas, exclusions and lifecycle independently of those model labels.
Membership editing cancels unfinished decisions bound to the closed interval;
rejoining creates a new interval and waits for the next user round to participate.
Memory writing and session compaction remain outside this delivery.
