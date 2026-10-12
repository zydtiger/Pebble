# Codex delegation

Characters keep conversation, roleplay, current-image understanding and the
existing `update_memory` tool. All additional grounding, external retrieval and
authorized work uses the single native `delegate_task` tool. This includes a
small current-fact lookup or calendar question. There is no routing model and no
persona web, calendar, shell, filesystem or work-skill tool. The character
explains actual saved evidence in its voice, preserving sources and limitations.
The final chat contract remains `{reply,state}`.

## Local connection and scope

Open **Codex tasks** from the sidebar to inspect or change the app-level
`[codex]` settings, independently of chat and participation connections:

```toml
[codex]
enabled = true
executable = "codex"
workspace = ""
writable = false
# model = "your-model"          # omit to inherit local Codex configuration
# effort = "high"               # omit to inherit local Codex configuration
```

Tasks are enabled by default when the installed CLI and login are usable.
Executable discovery checks PATH and conventional per-user/package-manager
locations, including `~/.nix-profile/bin`; the chooser also accepts an explicit
executable. Finder launches do not require an interactive shell configuration.
**Save & check connection** exercises the same resolved executable and checks
account status, models, workspace-scoped skills and MCP registrations. Missing
CLI, login and rejected model/policy configurations produce actionable errors.
No provider HTTP credential is added for this stdio connection. Login, enabled
skills and MCP configuration belong to the existing local Codex installation.
Pebble does not rewrite its global configuration.

Ordinary read-only lookups need no project selection. Each gets a managed
`data/codex-workspaces/<task-id>` directory under Pebble's existing data root and
Codex's read-only sandbox. File/code writes require an explicitly selected,
existing external workspace and **Allow writes**. The settings snapshot and
canonical workspace are frozen for the task and its followups. Native permission
requests are displayed before granting additional access. A workspace setting is
an execution policy, not a guarantee that every installed integration works.
Image generation, deletion and an always-running service are outside this scope.
External messages require explicit authorization in the actual user request.

A standalone app-server has the capabilities it actually discovers; desktop
registration alone does not establish usable desktop tools. Host permissions
are also launch-context specific. Calendar/notes routing should come from the
existing installed skills and available CLIs/MCP integrations, never personal
paths, calendar identifiers or duplicated Swift connectors in this repository.
A returned permission limitation is part of the result, not an invented lookup.

## Identity, execution and recovery

Rust creates the task, native submission/result and canonical origin binding in
one SQLite transaction before dispatch. The immediate tool result contains its
stable task ID, owner and accepted/existing status. Replayed calls and format
repairs reuse that recorded result; model-generated request keys cannot create
extra execution. Each independent task starts one Codex thread. Followups resume
that exact thread with the actual new user text and frozen scope. Both initial
and followup inputs include the immutable original initiating message, the
actual current followup user text (empty initially), the current delegated
objective and runtime-current local date/time/UTC offset. Native persona
followups preserve the current canonical user message separately from the
model’s instructions; card followups preserve the actual entered continuation.
Both retain the same thread and frozen scope across restart. Codex checks the
objective against the original and continuation texts, so authorization and
relative dates do not depend on a stale character prefix.

The stdio JSON-RPC adapter initializes a local child, starts/resumes threads,
starts turns and consumes actual notifications. It persists thread/turn IDs and
the dispatch boundary. States distinguish queued, running, needs input, needs
approval, cancellation requested, completed, failed, cancelled, interrupted and
uncertain. Only actual item events produce specific activity labels; otherwise
the card says Codex is working. There are no invented percentages, reasoning,
raw tool logs or repeated persona polling. The Rust manager owns subscriptions
and explicit reconciliation. A matching turn-level `error` with `willRetry=true`
keeps execution active with retry detail; `willRetry=false` immediately saves a
failed status. After explicit failure, the manager allows up to five seconds for
trailing final evidence and `turn/completed`, preserving the failure if they do
not arrive. This grace does not infer failure from inactivity. Stale errors,
ordinary failed tools and hook activity cannot end the current turn.

Cards show a title, **Codex**, reporting character, spinner/current state and
Cancel. Only running/cancelling execution shows progress; terminal states use
distinct static symbols and status colors. Explanation retries retain the task's
execution state, and empty results do not imply saved completed work.
Details are collapsed unless a question, approval or actionable recovery
needs attention. Completed evidence, actual source URLs and scoped artifact
links remain saved across navigation and restart. Citation control markers are
removed from display; unresolved markers receive an explicit source limitation,
and no missing URLs are fabricated. Multiple simultaneous native requests are
retained by request ID and presented in order. Duplicate, stale and resolved
requests do not replace or replay another pending question/approval. Command,
file, permission and supported connected-tool forms/authorization requests use
the native response shape; permission is granted only by an explicit answer.
Connected-tool forms preserve their required fields. Blank optional fields are
omitted from the response and the complete content is validated against the
original schema. String enum and titled `oneOf` choices preserve labels and
submitted values; array `items.enum` and titled `items.anyOf` choices use native
checkboxes and submit arrays. Primitive string, boolean, number and integer
fields remain available. Nested objects, arbitrary arrays and other unsupported
choice shapes are rejected explicitly before an answer is sent. Field descriptions,
numeric minimum/maximum, string length/format and choice-count constraints are
shown before submission and validated against the original schema, including
supported email/URI/date/date-time formats. Suggested defaults are displayed
without automatically answering; secret default values are hidden. Unsupported
constraints (including pattern, absent from the standard supplied schema) are
refused rather than hidden. Rejected answers remain editable until the request
actually changes. Native Codex questions remain required; their headers and
option descriptions stay visible while choosing, with free-text answers retained.
Command approvals display available parsed command,
file path and search details; terminal-input approvals identify the operation as
sending input, and managed-network requests show their complete host/protocol.
Missing operation details are stated explicitly. Pebble never
silently truncates an approval scope: native permission, command and file-change
approval summaries exceeding 8192 characters are explicitly refused before any
Allow control appears. File previews remain complete for this review decision.
Connected-tool HTTP/HTTPS authorization URLs belong only to the current native
request. They disappear when that request resolves and never become evidence,
artifact links or persona-report input. Only irreversible URL identities remain
for this task, so a later echoed authorization URL is removed from result text,
source links and outcome diagnostics, including saved-thread recovery and
followups. Ordinary returned sources remain available; original user messages
and frozen persona history are not rewritten.

**Stop** affects conversation generation. Task **Cancel** sends the actual Codex
turn interrupt and first records cancellation requested. Only a confirmed turn
outcome marks cancellation complete. If Codex acknowledges the interrupt but
provides no terminal confirmation within five seconds, the task becomes uncertain
and can be reconciled; it is never declared cancelled or resubmitted. Earlier completed effects are retained.
Cancel immediately removes pending questions/approvals and rejects queued or late
answers and requests; it grants no new permission while confirmation is pending.
Standard invalid request, method-not-found and invalid-params turn/start
rejections fail the task with the returned bounded reason; they do not trigger
reconciliation or automatic retry. Transport losses, internal errors and unknown
rejection codes remain uncertain because acceptance cannot be established.
Disconnects after dispatch are uncertain and never blindly resent. On restart,
accepted unfinished tasks that provably never attempted turn/start receive their
first dispatch using the same submission identity and frozen scope, resuming an
existing thread when present. Explicit cancellation and terminal failed/cancelled
tasks are never revived. Graceful shutdown at initialization or thread creation
also preserves never-submitted work and any returned thread identity; internal
shutdown is distinct from a user's durable cancellation request.
Already-submitted unfinished work is marked
interrupted/uncertain and only reads the saved thread when possible. If a turn/start response was lost before its turn ID was saved,
recovery matches the saved submission to the user message’s echoed `clientId`,
not its server-generated item ID. Unmatched submissions remain uncertain and
are never resent. **Reconcile** reads actual
outcomes, and followup is unavailable until execution and reporting permit it.
Pebble manages children while open and interrupts/closes them on shutdown; it
does not promise that work continues after quit.

## Reporting and visibility

Each followup starts with empty current-run sources/artifacts; previous run
evidence remains in its historical snapshot.

Execution and character reporting are separate durable lifecycles. Rust saves
evidence before admitting one completion report into the original character's
normal queue. A configuration or launch error after report admission immediately
fails that pending explanation and its bubble, keeping the saved outcome ready
for **Retry explanation** after repair without restarting or executing work again.
Ordinary chat and explicit fresh contexts can run during task
execution; reporting uses the current accepted context/state. The report gets
stored evidence, has no active work or memory tools, and publishes through the
normal validated reply/state transaction. A failed explanation exposes **Retry
report**, which uses the stored result without rerunning Codex. Failed, uncertain
or interrupted execution can also be explained accurately without pretending
that the requested work completed. Each report freezes its outcome revision and
evidence. A later reconciled outcome receives its own report; earlier queued or
streaming reports cannot publish over that newer outcome. Repeated callbacks do
not report the same revision again. Ordinary bubble Retry is unavailable for
task reports; use the card’s explanation recovery control, including after a
fresh context.

The origin room and original membership interval must still be eligible when
publication occurs. Removing/rejoining a character does not revive the old
interval. An invalid destination retains its card/result and receives no public
reply or automatic private-to-group reroute. The original membership interval
cannot be restored, so this state offers no explanation retry. Same-task followup
stays unavailable while its explanation is pending or queued, matching the core
API. A private task result is available
only to its owning character; other characters receive only normally published
public text/artifacts. The shared character-context disclosure limitations in
[group rules](groups.md) still apply. A late report does not reopen an old group
round or permit an unbounded reply chain.

For group messages the default is one shared task per canonical user message.
The first actual native delegate call in scheduled speaker order atomically
claims the task and reporting owner; a speaker who only chats cannot block it.
Other same-assignment calls reuse that actual stored task/owner, including calls
with different model-generated keys and calls that continue different prior
tasks. Every new user message claims its own partition before either new work or
a same-task followup is dispatched; the task’s original origin remains saved.
A conflicting objective cannot silently
overwrite the task or start another; the character receives its existing task
and can ask for an explicit separate request.

When deliberately assigning different work, select multiple characters in
**Mention**, then choose **Separate tasks for mentioned characters** in that
menu. This per-message draft choice and selected mentions are frozen into the saved
message. Shared mode is the default for the next message after successful
acceptance; failed sends and newer in-flight edits preserve their draft choice. Each selected character has one independent partition; unmentioned
speakers cannot invent additional partitions. Ordinary shared mode needs no
extra composer control.

## Persistence and validation

Schema 13 adds task, native submission, assignment and historical-run records,
plus explicit delegation capability and message assignment fields. Schema 12
upgrades transactionally without rewriting old native prefixes/history. Existing
contexts retain their frozen capability contract; an explicit fresh context
activates delegation. New contexts include the delegation contract.

Deterministic [core fixtures](../crates/pebble-core/tests/delegation.rs) exercise
both native protocols, prompt return/replay, dispatch loss, stale callbacks,
concurrent questions, approvals, same-thread followups, cancellation, restart,
report retry, continued chat/context changes, shared/explicit group assignments
and membership privacy. A [followup fixture](../crates/pebble-core/tests/delegation_followup.rs)
checks original/continuation/objective payloads through both native APIs, card
followups, reloads and frozen scope. Opt-in probes use isolated synthetic roots; ordinary
checks never access personal calendars or notes.

Local evidence is kept under ignored
`build/validation/codex-delegation-2026-10-11/`, including raw command/exit/duration
records, protocol schema and native renders. Installed Codex 0.160.1 authenticated
successfully, exposed models/skills/MCP registrations, completed bounded official
web research and a synthetic scoped artifact write. Both selected chat protocols
completed native delegation and explained the saved web result. Model/effort
were inherited unless a test explicitly supplied them.

The calendar status-only probe discovered the installed routing skill and CLI,
but its read-only child returned `NotDetermined`, while a direct host status
check returned `FullAccess`. No calendar/event contents were read and no
permission request was made. A bounded `doctor --json` comparison also returned
`NotDetermined` for Calendar/Reminders in the child, despite embedded usage
descriptions being present and both commands exiting zero. These diagnostics
do not establish whether the sandbox or responsible-process TCC attribution
causes the difference; suggested authorization commands were not executed.
Calendar access in this execution context is **not
verified operational**; the limitation must be resolved through the actual
launch/permission context, without automatically broadening the sandbox.

Local native-request refusals remain in the saved outcome and persona report,
including after live completion or thread recovery. Failed turns include the
returned error message (at most four lines and 1024 characters); raw error
details and stacks are not displayed. A local worker-start failure releases its
owner and records failed pre-dispatch work, or preserves uncertainty when the
worker was only meant to reconcile an existing execution.
