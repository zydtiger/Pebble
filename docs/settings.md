# Settings and recovery

Native settings are a Rust read/validate/write API over local TOML and Markdown.
The app defaults form edits each connection and its required context/output
limits; the character form adds name/subtitle/SOUL/MEMORY and whole connection
overrides. An absent character connection inherits the entire respective app
connection, including both limits. An explicit override requires its own limits.
An absent app chat connection selects the demo.

## Safe writes and credentials

Settings reads expose has_api_key and api_key_env, never an inline key. Swift
shows secure replacement input with explicit Keep/Replace/Clear/Environment
choices. Keep preserves existing fields exactly; an empty replacement is
rejected rather than clearing a key accidentally. Clear removes the environment
reference and writes an empty inline key; Environment removes the inline key.
Rust validation still rejects simultaneous nonempty inline/environment keys,
invalid URLs/protocols, and invalid budgets. Credentials remain absent from
history, call records, and error text; replacement debug output is redacted.

Queue acceptance and writes serialize. App-default writes require all personas
idle; a character write requires only that persona idle. Pending participation
decisions and still-admittable group slots also block settings writes for their
character (or app defaults). Before any authoritative
mutation, candidate overlays validate the whole workspace and dependent inherited
budgets. toml_edit preserves unrelated fields/comments and changed string-value
decoration where practical. Turning off a chat override removes its entire table,
including unknown fields inside it, because an incomplete chat table is invalid.

Every candidate file is written and synced to a temporary sibling first. Once
all staging succeeds, atomic replacements publish SOUL, MEMORY, then TOML.
Existing file permissions are retained. A staging/validation failure changes no
authoritative file. The three-file character save is not a cross-file transaction:
if a later replacement fails, the error lists already saved files and asks the
user to inspect/reopen. Unpublished temporary files are cleaned up. There is no
hidden rollback that could overwrite a concurrent manual edit.

Saving SOUL/MEMORY or budgets does not rewrite an existing session prefix.
Explicit fresh context reviews outgoing knowledge with current MEMORY before publishing current files; its saved-effect recovery is described in [memory](memory.md). Model/root/protocol changes use
the existing connection-boundary behavior. Broken manually edited configuration
may require advanced file access; the settings UI does not implement a repair
parser. Drafts are not persisted across app restart in this release.

## Explicit retry

Eligibility is derived from the latest call/reply for a persona across private
and group rooms,
including its retry chain. It must be failed/cancelled/interrupted and there must
be no queued/active work for that persona. Earlier unsuccessful replies cease to
be eligible when a later call exists. Other personas can continue independently.

Retry appends one new reply and queued model_call with retry_of, reusing the
original user message and receipt event. Old partial/error/status/cutoffs remain
unchanged. There is no second user bubble or received event. Acceptance and
activation retain the same durable queue and cancellation rules as normal sends.

When the current compatible active session matches the nearest frozen ancestor,
activation reuses its exact input/event cutoff. Keys, budgets and request parameters resolve from the
new attempt's current runtime configuration. An intervening retry cancelled before
activation does not prevent reuse of an earlier frozen ancestor. New native output
is published once only after a successful completion.

For a never-activated input, retry makes the receipt consumable once after queued
predecessor output has been accepted. For a changed connection or explicit fresh
context, retry explicitly admits the selected original receipt even if it precedes
the new boundary; it does not import unrelated earlier events. Its native
representation prevents the next ordinary turn from importing it again.

Startup marks retained queued/active calls interrupted. The transcript exposes
saved failure details and current retry eligibility after reopen. Nothing is
resent automatically. Repair settings or choose a fresh context deliberately,
then retry. The native UI keeps earlier attempts and unsent room drafts intact.


## v0.1.1 state contract

Character config also carries explicit `[response.initial_state]` and its
separate `response_schema.json` defines the response contract. Existing settings
saves preserve those fields/files. Structured contexts show typing until a
validated atomic acceptance; failed attempts save errors but no candidate text.
Legacy contexts retain their old streaming behavior until confirmed fresh
context. The separate **State & response** panel inspects current state, edits schema and
initial defaults, and applies explicit fresh-context keep/reset choices; see
[state lifetime, publication, and recovery](state.md). The Responses connection
form exposes the standard text.format and explicit llama.cpp response_format
dialects described in [providers](providers.md). Native protocol switches remove
Responses-only fields; ordinary credential saves preserve the chosen dialect.

## Independent participation settings

App defaults and character settings link to a separate **Participation model**
sheet, also available in each group. It edits `[defaults.classifier]` or the entire
persona `[classifier]` override, leaving chat/SOUL/MEMORY/budgets untouched. Turning
off a character override inherits the app classifier; turning off the app
classifier leaves manual mentions. Credentials use the same explicit secure
Keep/Replace/Clear/Environment choices and per-file atomic publication rules.

Responses offers its standard/llama schema dialect. Both protocols expose the same
concrete request-parameter JSON editor as chat settings. Protocol switches remove
the inapplicable schema dialect; incompatible request parameters fail validation
and remain in the form for explicit correction. Classifier changes do not retire
chat sessions or reset state. See [request contracts](participation.md).

## Concrete request parameters

Each complete connection may contain a `request` table:

```toml
[defaults.chat.request]
temperature = 0
top_p = 0.9
[defaults.chat.request.chat_template_kwargs]
enable_thinking = false
```

The same shape applies to `[defaults.classifier.request]`, persona `[chat.request]`
and `[classifier.request]`. A persona override replaces its entire connection,
including parameters; there is no merge with app-default values. The native
**Request parameters · JSON object** editor reads and saves that table directly.
Numbers, booleans, strings, arrays and nested objects keep their types. `{}` omits
optional parameters; `false` and `0` are explicit values. JSON `null` and integers
outside TOML's signed 64-bit range fail before publication. TOML date/time values,
including nested values, are rejected before conversion; quote date text when a
JSON string is intended. Use omission for an
unset value. Named profiles, presets and a separate parameter registry are absent.
An obsolete `request_profile` field is an actionable configuration error: remove
it and write its intended concrete values. No automatic conversion occurs.

Rust validates known sampling types/ranges, protocol-specific fields and
contradictory thinking controls. Enablement, effort and budget are separate:
compatible controls can be combined, but disabled/none/zero conflicts with
active effort or a positive/unlimited budget. Backend-specific effort strings and
extension keys remain unverified; the editor states that backend/model support
must be checked, and HTTP success alone proves no behavioral effect.

The local `request` namespace is flattened into the final HTTP body. Nested
`chat_template_kwargs` stays nested; Responses `text.verbosity` shares `text`
with Pebble's protected `text.format`. Parameters cannot override identity,
credentials, inputs, streaming, output limits (including `n_predict`), native
tools, schema/parser contracts, Responses privacy/replay, or app state/control
fields. `context_len`, `max_output_tokens`, protocol and schema dialect retain
their dedicated controls. Grammar, tool-parser and reasoning-format overrides
are rejected because they can break the native reply contract.

The final composed body is budgeted before HTTP for chat, every participation
phase, repair, tool continuation and memory consolidation. Accepted work freezes
its effective parameters; native saves remain idle-only. Explicit Retry uses
current settings with its existing compatible input boundary. Sampling and the
per-request `enable_thinking` switch preserve active history. Other template
options may change history formatting, so their exact values participate in the
existing durable connection binding and cause a fresh context when changed.
No database migration or duplicate parameter store is required.
