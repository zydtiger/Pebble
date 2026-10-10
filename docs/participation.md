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
request_profile = "qwen_non_thinking"
```

Existing authored config is never overwritten or extended silently. An absent
classifier means manual mentions with a clear setup status. A persona `[classifier]`
replaces the entire app classifier connection, independently of `[chat]`; it
requires model, API root and explicit protocol. Credentials are independent and
support empty/no authentication, inline key or environment reference. Native
**Participation model** controls expose app defaults and character overrides.
Disabling a character override inherits app defaults. Saving requires idle
replies, decisions and still-admittable slots in the affected scope; validation precedes atomic TOML
replacement. No chat session or state boundary changes.

Both Chat Completions and Responses use temperature 0, streaming false, a
64-token output budget and required-boolean-only strict JSON schema. Chat sends
`response_format.json_schema`. Responses sends `store=false`, `max_output_tokens`,
and standard `text.format`, or the explicitly selected `responses_schema_format =
"response_format"` llama-compatible extension. There is no provider/hostname/model
sniffing and no alternate-model retry. `request_profile = "qwen_non_thinking"`
explicitly sends Chat-only `chat_template_kwargs.enable_thinking=false`; it is
invalid with Responses. Without this setting, standard endpoints receive no Qwen
extension. Native protocol switches remove inapplicable profile/dialect fields.

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
- Up to eight recent complete messages delivered to this persona in the target
  group, and eight of its eligible events from other rooms. Each text excerpt
  is bounded at 500 bytes, with source event/message/room/speaker identity.
- Chronological excerpts including accepted same-round predecessors and actual
  already-represented persona knowledge. No retrieval
  or memory consolidation is implied.

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
(up to 8,000 UTF-8 bytes), without SOUL/interests. Its connection comes from the
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
PEBBLE_CLASSIFIER_API_ROOT=http://beluga:4014/v1 \
PEBBLE_CLASSIFIER_MODEL=Qwen/Qwen3.5-2B \
PEBBLE_LIVE_API_ROOT=http://beluga:4013/v1 \
PEBBLE_LIVE_MODEL=gemma-4-31b-uncensored \
cargo test -p pebble-core --test participation_live_endpoint -- --ignored --nocapture
```

Optional `PEBBLE_CLASSIFIER_REPORT` must be an absolute path in an existing
directory. If proxy routing fails, retry only that command with
`NO_PROXY=beluga,127.0.0.1,localhost`.

The bounded selected-service run produced TP 5 / FN 0, then FP 0 / TN 3 across
eight examples. With two concurrent private Gemma calls, all four group decisions
completed in 83 ms, each admitted one chat reply, and all replies completed.
All twelve Qwen requests returned strict boolean JSON with zero reasoning tokens.
This is an integration regression using changed compact/attributed input
organization and unchanged labels, not an independent generalization estimate
or a latency guarantee. The initial report write used a relative path and failed
after all behavior assertions; the complete JSON was recovered from its raw log
without repeating service requests.

The earlier combined intent-and-permission prompt passed 7/10 frozen development
examples. Splitting the passes scored 9/10 (TP 3 / FN 1 / FP 0 / TN 6), with all
15 requests schema-correct and zero reasoning. The remaining miss applied another
member's silence restriction to an allowed target. These are development probes,
not an independent accuracy estimate or a universal permission guarantee. The
runtime uses the exact split prompts and deterministic routing; integration tests
verify schemas, exclusions and lifecycle independently of those model labels.
Membership editing, memory writing and session compaction remain later scope.
