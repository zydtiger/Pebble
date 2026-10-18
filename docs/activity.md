# Private activity and heartbeat

Private activity is opt-in per character and runs only while Pebble is open.
Enable it from the character's chat header. The popover shows the current
objective, a short saved progress summary, running task IDs through the existing
task monitor, the next local check time and a pending return commitment. It also
offers enable/disable, stop, a daily check budget and local quiet hours. Disabling prevents
new decisions and publication; an already accepted task may finish and save its
result. Stopping cancels current autonomous tasks and pending intents. Chat
interruption preserves accepted external work.

## Scheduling and time

Each persona's `personas/<persona-id>/config.toml` is the sole source of heartbeat
settings. The UI writes the same table by atomic file replacement, preserving
unrelated configuration and comments:

```toml
[heartbeat]
enabled = true
daily_wake_budget = 4
quiet_start = "23:00"
quiet_end = "08:00"
```

Newly created characters (including the first starter character) receive
`enabled = false`, `daily_wake_budget = 4`, and local quiet
hours from `23:00` to `08:00`. These are initial file values, not app-level
inherited defaults; existing character files are left unchanged.

Omit both quiet-hour keys to disable quiet hours. Times use local 24-hour `HH:MM`
notation, and may cross midnight. An omitted table defaults to disabled,
four checks per UTC day and no quiet hours. Invalid fields, values or
incomplete quiet-hour pairs are rejected. **Save schedule**, enable/disable and stop all write this file; stop writes `enabled = false` in addition to cancelling current work.

File edits are read on the next scheduler pass (normally within a second while
the app is open), and again before private effects or publication are accepted.
The header updates through normal activity observation; reopen the popover to
reload its editable fields. Setting `enabled = false` prevents
new evaluations and publication, and cancels unaccepted private generation.
Already accepted Codex tasks may finish and save evidence; use **Stop current
activity** to cancel those tasks too. Disabling preserves progress and pending intents for re-enabling. Configuration edits do not reset daily use.
Removing a quiet-hour or exhausted-budget restriction allows the next scheduler
pass to re-evaluate that hold, without resetting consumption.

SQLite stores only runtime data: due time, reason, daily wake consumption, and
exploration progress, tasks and intents. Schema 21 migrates existing database
settings into TOML only when the file has no `[heartbeat]` table, then removes
the old policy columns/table in favor of `persona_activity_runtime`. Explicit
file settings win. Publication precedes the schema commit, so a retry preserves
already migrated tables. Existing usage, dates and due times are retained.
The former `paused` field is removed on startup and on UI saves; an old
`paused = true` becomes `enabled = false`, so upgrading never resumes a paused
character. Legacy database settings follow the same rule.

The default
budget is four private evaluations per UTC day, configurable from 1 to 24.
Quiet-hour bounds use local time and must be distinct. A due wake is coalesced
while a persona has active chat, maintenance or another private evaluation.
Missed time does not create a backlog of periodic calls. Restart closes unfinished
private calls and preserves accepted task identities for normal reconciliation.

The model may propose the next check between 20 and 1440 minutes. A null private
interval falls back to six hours. Failed or interrupted private calls retry no
earlier than 30 minutes. Saved task outcomes can wake the persona sooner, subject
to quiet hours, the daily budget and queue admission. This is a wake budget, not
a token or currency limit: a wake can include a bounded tool exchange and one
separately admitted return reply. There is at most one active research task per
activity and one accepted task per activity step.

Each model activation freezes UTC, local date/time and offset, prior user receipt,
last actual persona publication, their elapsed seconds, and current-message queue
delay. Background work does not count as user contact or persona publication.
Retries replay the frozen input. Time can inform characterization but does not
establish rejection, neglect or user intent.

## Contracts, plans and publication

Persona-authored `response_schema.json` still defines `reply` and `state`. New
sessions derive a purpose-aware runtime contract without editing that file:

- Chat output: `reply`, complete `state`, nullable `activity` plan proposal.
  Missing activity metadata is accepted as a no-op for existing local fixtures.
- Private output: complete `state` and `activity`, with no `reply`. Activity
  contains a concise progress summary, phase, next interval, nullable `plans`,
  nullable `share_intent_id`, and `observed_outcomes` acknowledgments containing
  exact task IDs and outcome revisions from the frozen input.
- Admitted return: `reply` and complete `state`, without new tools or plans.

Plans hold one main objective, up to three future ideas, five optional share
intents and three return commitments. Rust assigns intent IDs. Updates and
fulfillment must reference saved IDs owned by the persona and destination.
Future ideas are an explicitly replaced backlog for later model decisions, not
parallel tasks. Caps never silently evict promises. Commitment creation commits
atomically with the visible promise; private turns can create optional intents
but cannot invent unpublished commitments or silently dismiss a promise.

Chat context labels each pending intent with `eligible_for_fulfillment`. Invalid
plan effects receive up to two corrections within the existing three-candidate
reply limit. If correction still fails, Rust publishes a valid reply/state while
rolling back the entire rejected activity proposal with a savepoint. A retained
valid reply also survives transport or format failure during plan correction;
user cancellation still wins. Native history records a runtime rejection next to
the accepted output so later turns cannot assume the rejected effects happened.
Accepted memory operations are not repeated. Database/configuration errors and
invalid reply/state remain actual failures; this is not a blanket error bypass.

An intent has an earliest contact time and either a `ready` or
`activity_complete` condition. The latter requires actual terminal task evidence
to have been privately assimilated, including any failure or limitation. The
model can request contact using a saved intent ID. Rust then admits at most one
separate reply, checking destination, quiet hours, enabled status, queued user messages
and a 20-minute activity-publication cooldown again at commit. No placeholder is
created. Only successful publication fulfills the intent. Failure or interruption
leaves it pending for a later decision. Autonomous contact currently targets
the persona's direct chat; group activity rounds are not implemented.

User input cancels private HTTP generation and takes the same serialized persona
queue. The next chat envelope includes the durable objective and pending intents,
so the character can answer the user first, explain its actual activity when
relevant, and record a return promise. A private decision and chat never write
accepted persona state concurrently.

## Existing Codex delegation

No new model tool is introduced. Private turns reuse `delegate_task` with a
freely selected title and objective, a descriptive request key and `task_id: null`.
There is no topic catalog. Rust forwards this brief, not the full persona context.
The character must include only relevant context and exclude unrelated private
chat, SOUL, MEMORY, state and secrets. Model instructions do not establish user
consent for external actions.

Opting in permits exploration using the existing app-level Codex task settings,
available MCP tools and the independent task workspace. Settings are frozen at
task admission exactly as for user-assigned tasks: `writable` selects
`workspace-write` or `read-only`, and both new and resumed threads use
`on-request` with `auto_review`. MCP registration does not block dispatch.
Autonomous threads explicitly request `config.web_search = "live"` on start and
resume, without editing global Codex configuration. Managed policy, provider
capability and actual integration permissions still apply; errors do not trigger
an unrestricted fallback. External messages, purchases, deletion and unrelated
private-data disclosure are not authorized by enabling background activity.
Previously saved experimental tasks with the old public-search-only scope remain
readable. They may reconcile existing results but cannot start or resume execution
under expanded permissions; start new activity instead.

Existing permission and input deferral stops the turn and preserves its actual
limitation; the persona may propose a return intent to ask the user. A private
wake cannot invent answers or continue a task on the user's behalf. An actual
user reply can continue the saved task through ordinary chat delegation.

These tasks freeze `origin=autonomous` and `delivery=private_observation`.
Their real progress remains visible in the ordinary task monitor. Completion
saves evidence and wakes private assimilation instead of enqueuing a room report.
A frozen `(task_id, outcome_revision)` list ensures that a result arriving during
an evaluation is not accidentally acknowledged. Each evaluation receives one
bounded outcome excerpt; the full result remains in the task record. The private
output must explicitly acknowledge exactly those IDs/revisions and supply a
nonempty outcome summary. Schema, acknowledgment and plan validation run before
acceptance, and the commit boundary verifies them again. Task receipt, private
state, plans and native output commit atomically. A newer outcome revision stays
queued for its own review. Acknowledging a failed task records its limitation;
it does not change that task into a success.

Invalid private JSON, acknowledgments or plan effects receive at most two
correction requests using the same frozen evidence and state, within a total
180-second private operation deadline. Corrections disable tools and create no
extra heartbeat wake or chat message. User input may cancel any correction.
After exhaustion the evidence remains `private_queued`; a later eligible wake
can review it again under the existing quiet-hour and daily-cap rules. Successful
verified acceptance sets `private_assimilated`. Task execution is never repeated
to retry assimilation or message publication.

## Storage and file ownership

`activity.rs` owns the scheduler and public snapshots. `chat/activity.rs` owns
private generation and queue cancellation. `activity/settings.rs` owns the validated TOML policy and one-time migration;
`storage/activity/` separates
scheduling, private calls, delegation, plan effects, publication and frozen time.
Schema 17–20 add activities, events, steps, intents, private calls and
idempotent task submissions. `model_items.activity_call_id` stores private native
history in the same persona session without inventing a `model_calls` reply.
Swift `Activity.swift` renders typed FFI snapshots and native controls; it does
not own timers, admission, state transitions or task effects. Only an actual new
publication refreshes chat and enters existing notification deduplication.

Old session prefixes remain byte-for-byte frozen. Enabling activity on a legacy
reply/state-only session waits for a fresh context boundary. Manual fresh context
or automatic rollover creates the purpose-aware successor. Private-only native
history can also trigger automatic memory review. Automatic rollover retains up
to ten **already-observed chat messages by ID**, with the existing whole-message
budget reduction. Private steps, tool results and progress events consume none
of those ten slots. Original text and image assets remain at their original
identities; recent history hydrates those references. Plans, intents, schedules
and task evidence persist independently of rollover. Manual fresh context keeps
its existing no-carry behavior.

## Validation and remaining scope

`storage/activity/tests.rs` covers atomic intent limits, publication, suppression,
task admission/deduplication, late result revisions, quiet hours, restart budgets,
shared native history and chat-only rollover. `tests/activity.rs` exercises the
real scheduler, both HTTP protocols, isolated app-server subprocesses, private
assimilation and its bounded repairs, configured MCP, both workspace policies
and user interruption during repair. `tests/intent_recovery.rs` checks semantic
plan correction and exhausted/failed correction on both protocols, retained
chat/state, once-only memory writes and explicit runtime rejection in replay. Run `just check-rust`,
`just check-bridge`, `just build` and
`PEBBLE_RENDER_ACTIVITY_ONLY=1 just preview` for core/native validation.

An opt-in real-backend check is available with
`cargo test -p pebble-core --test activity_live -- --ignored --nocapture`.
It uses synthetic persona HTTP replies and isolated Pebble data, but runs the
actual scheduler, delegation manager and installed Codex. It requires a real
`webSearch` progress event, a source link, private assimilation and no chat bubble.
It does not claim to validate a live persona model's autonomous topic selection.

`heartbeat_end_to_end_live` additionally uses both a real persona endpoint and
real Codex. Select the persona with `PEBBLE_LIVE_API_ROOT`, `PEBBLE_LIVE_MODEL`,
`PEBBLE_LIVE_API`, and `PEBBLE_LIVE_API_KEY`; optional context/output limits and
request parameters use `PEBBLE_LIVE_CONTEXT_LEN`, `PEBBLE_LIVE_MAX_OUTPUT_TOKENS`,
and `PEBBLE_LIVE_REQUEST_JSON`. Codex model/effort can be selected with
`PEBBLE_LIVE_CODEX_MODEL` and `PEBBLE_LIVE_CODEX_EFFORT`. Run:

```sh
cargo test -p pebble-core --test heartbeat_end_to_end_live -- --ignored --nocapture
```

This isolated test enables two wakes and disables quiet hours only for its
temporary character. The scheduler triggers a bounded public research task;
the test sends a chat while that task is running, then requires a real web-search
event, source URL, completed task, exact private assimilation, and only the two
ordinary chat messages. A DeepSeek/Codex run passed on 2026-10-17 in about 24 seconds.
Its topic is prescribed for reproducibility; it does not measure spontaneous
topic quality or change existing persona schedules or conversations.

Future work includes specialized shared games and group autonomous participation. No background OS service, external messaging,
new native model tool or release is part of this increment.
