# Structured replies and persona state

v0.1.1 adds validated private-chat responses. New contexts require a complete
`{"reply":"...","state":{...}}` JSON object. The UI shows **Typing…** until Rust
validates and commits it, then displays only `reply`. Candidate JSON and opaque
reasoning never enter the transcript. Accepted native history keeps the complete
JSON response and protocol-native reasoning for exact replay.

## Definition and lifetime

Each character directory contains `response_schema.json`, a JSON Schema with an
object envelope, exactly the required `reply` (string) and `state` (object)
properties. Both envelope and state use `additionalProperties: false`, and every declared state field is required. Customize the state properties,
required fields, bounds, and descriptions. Rust uses JSON Schema validation;
external HTTP/file references are disabled, while local references are allowed.
The selected provider may support a narrower schema subset than Rust.

Explicit initial state belongs in the character's `config.toml`:

```toml
[response.initial_state]
affinity_user = 20
mood = "calm"
```

The bundled schema defines `affinity_user` as an integer from 0 to 100, directed
toward the local user: 0 is distant, 50 familiar, and 100 deeply close. It starts
at 20. Only meaningful relational interactions warrant a change; ordinary
messages need no increase. `mood` is a nonempty string of at most 80 characters,
initially `calm`. These fields describe a present condition, not durable facts or
an automatic memory archive. Numeric bounds impose no per-turn transition limit.

SQLite's `persona_states.current_state` is the single authoritative accepted
snapshot. Initial values initialize it once; editing them does not reset it.
State belongs to the persona and survives restart, model/protocol changes, and
**Start fresh context**. No current-state JSON file is maintained.

A session freezes its schema, semantic response rules, SOUL/MEMORY, and starting
state in the prefix. Subsequent attributed input envelopes include the current
accepted state when the queued turn activates. Each call also saves that snapshot
as `state_before`. Accepted outputs carry complete subsequent state snapshots.
Editing the schema cannot reinterpret an existing session or historical output;
only an explicit new-context boundary loads it. A fresh context rejects a schema
incompatible with the current accepted state, preserving both the state and old
active session. State/schema inspection, editing forms, and deliberate reset are
v0.1.2 work; advanced file editing remains available.

## Acceptance and recovery

Rust checks complete JSON syntax, schema constraints, nonempty reply, and state
object shape before publication. A short SQLite transaction independently
revalidates native output and checks the frozen prior state, then publishes the
visible reply, next state, native items, sent event, links, and terminal call
together. A later SQL failure rolls all of these back. The next queued call
observes only committed state.

A completed candidate with malformed JSON or invalid schema may be regenerated
twice (three candidates total). Each attempt uses the same saved request and
accepted state, with one transient validation-feedback input. Rejected native
output and candidate state are never added to conversation history. Durable
`model_calls.validation_errors` stores rule/path diagnostics, not rejected text
or reasoning. The transient feedback also counts against the context budget.

In structured contexts, transport/SSE errors, refusal, cancellation, truncation, and exhausted output
budgets are terminal failures, not format-repair triggers. Structured failed,
stopped, or interrupted bubbles have no provisional reply text and leave state
unchanged. Explicit latest-reply **Retry** remains separate: it creates another
call without duplicating the original user message, reusing the nearest compatible
frozen input and state snapshot. Nothing resumes automatically after restart.

## Upgrading a v0.1.0 root

Schema versions 1–6 migrate transactionally to 7. Existing transcript IDs,
message contents, native bytes, sessions, and files remain preserved. Registered
legacy characters without durable state receive a missing default schema and
initial-state config fields; existing definitions, comments, and unrelated TOML
values are retained. Config publication uses atomic sibling replacement. If
initialization fails, repair the named file and reopen; no accepted state has
been reset. Missing contract files after initialization report an error rather
than being silently regenerated.

An existing v0.1.0 model session has no structured contract. It remains active
with its frozen plaintext behavior and native replay. A persistent **Legacy model
context** notice explains that structured state requires **Start fresh context**.
That existing action asks for confirmation and makes the explicit boundary;
earlier transcript/native history stays saved and excluded from the new context.
A user-selected model/protocol/root change already establishes a fresh context
and activates the contract too. Legacy replies do not mutate persona state or
receive provider schema constraints. New characters and first sessions use the
structured contract immediately.

Legacy Responses refusal text remains a completed visible response with native
replay, as in v0.1.0. Legacy Chat Completions requires nonempty content; a
refusal-only result fails, while content accompanied by a refusal field retains
the original plaintext behavior. Structured contexts classify either protocol's
refusal as a terminal failure without accepting native output or state.
