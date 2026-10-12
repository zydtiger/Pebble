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
writable = true
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

Each new task gets its own `data/codex-workspaces/<task-id>` directory
under Pebble's application data root. There is no configurable external workspace
in app settings or the native interface; the former `[codex].workspace` key is
no longer accepted. **Allow writes in the task workspace** defaults on and selects
Codex's `workspace-write` sandbox; turning it off explicitly selects `read-only`.
Existing explicit `writable = false` settings remain read-only. Durable task
snapshots retain their saved workspace paths and execution settings for restart
and followups, including tasks accepted before this setting was removed.
Changing app defaults never widens an existing task's scope.

**Approve for me** routes eligible permission requests to Codex's separate automatic
reviewer. Both `thread/start` and `thread/resume` explicitly send
`approvalPolicy = "on-request"` and `approvalsReviewer = "auto_review"`.
Baseline execution stays sandboxed; the reviewer decides whether an eligible
action may run beyond that sandbox. Rejected or unsupported policy settings fail
visibly, with no fallback to manual approvals or unrestricted execution.
An automatic approval does not guarantee host integration access. The sandbox
policy is not a guarantee that every installed integration works.
Image generation, deletion and an always-running service are outside this scope.
External messages require explicit authorization in the actual user request.

A standalone app-server has the capabilities it actually discovers; desktop
registration alone does not establish usable desktop tools. Host permissions
are also launch-context specific. Calendar/notes routing should come from the
existing installed skills and available CLIs/MCP integrations, never personal
paths, calendar identifiers or duplicated Swift connectors in this repository.
A returned permission limitation is part of the result, not an invented lookup.

## Identity, execution and recovery

A safely identified single `delegate_task` call with invalid arguments returns
specific validation feedback as a native tool result matched to its call ID.
The same chat model gets at most two corrective retries (three invalid attempts
in total). All four fields are required, including explicit `task_id: null` for
new work or an actual existing task ID for continuation. Rust does not invent
missing values or change authorization. No task or submission exists before
valid arguments are accepted. Malformed identities, mixed/multiple or unsupported
calls, refusals and transport/truncation failures remain hard failures.

Corrections share the existing 180-second operation deadline and context bounds,
but do not consume the final response's three-candidate validation budget.
Each rejected call and its error result are saved together in existing native
history, including exhaustion or cancellation. Explicit reply retries replay
that balanced history; once a task is accepted, they only repair its chat
explanation and cannot dispatch it again.

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
model’s instructions and retain the same thread and frozen scope across restart. Codex checks the
objective against the original and continuation texts, so authorization and
relative dates do not depend on a stale character prefix.

The stdio JSON-RPC adapter initializes a local child, starts/resumes threads,
starts turns and consumes actual notifications. It persists thread/turn IDs and
the dispatch boundary. States distinguish queued, running, needs input, needs
approval, cancellation requested, completed, failed, cancelled, interrupted and
uncertain. Only actual item events produce specific activity labels; otherwise
the monitor shows the saved execution state. There are no invented percentages, reasoning,
raw tool logs or repeated persona polling. The Rust manager owns subscriptions
and explicit reconciliation. A matching turn-level `error` with `willRetry=true`
keeps execution active with retry detail; `willRetry=false` immediately saves a
failed status. After explicit failure, the manager allows up to five seconds for
trailing final evidence and `turn/completed`, preserving the failure if they do
not arrive. This grace does not infer failure from inactivity. Stale errors,
ordinary failed tools and hook activity cannot end the current turn.

A small circular activity button is fixed at the transcript viewport’s top-right,
below the private/group header. Its compact dropdown lists only tasks executing
across the app: running or awaiting cancellation confirmation, without a pending
native interaction. Both the button and each row show a progress circle.
Queued, blocked, failed, interrupted, uncertain, completed and cancelled tasks
are hidden; the button disappears when nothing is executing. Outcomes and failures
are explained by the character through chat, with saved task evidence retained.
Rows contain only the brief task description and reporting character/status;
there are no task cards in the transcript, task followup fields, approval/question
forms, retry or reconciliation controls. The button has a subtle shadow and dims
on hover. Outside clicks and Escape close the dropdown; reduced motion disables
its transition. Explanation delivery never revives a hidden task or animates
otherwise inactive execution.

Codex is instructed to begin each run with one short public commentary describing
its task. Rust captures the first nonempty commentary, normalizes whitespace and
bounds the display to 96 characters; invalid long/link-bearing descriptions keep
the initial persona title. Later commentary, reasoning and tool logs never become
monitor content. The description is saved in the existing task record and resets
for a same-thread followup; no database schema migration is needed.

Native clarification requests are deferred without supplying an answer. Rust
retains a bounded question with its option consequences for the reporting character,
interrupts the turn and waits up to five seconds for actual terminal confirmation.
The character asks in ordinary chat; the user’s subsequent message can continue
the same task using `delegate_task(task_id)` after reporting finishes. Secret
questions and connected-tool forms/authorization requests instead produce a
configuration limitation; chat must not solicit secrets or authorization tokens.
Codex handles eligible automatic reviews internally. Matching
`item/autoApprovalReview/started` and `item/autoApprovalReview/completed`
notifications keep the task running; approval, denial, timeout or abort never
substitutes for the actual turn outcome. Bounded typed denial/timeout/abort
rationales remain in saved limitations and the character's report, including
after completion or recovery. Unstructured `guardianWarning` messages also
announce successful reviews, so they are ignored in favor of typed outcomes.
Stale thread/turn review events are ignored.

Any manual command, file or permission request that still reaches Pebble is
explicitly declined. Pebble requests interruption and explains the saved outcome;
no approval remains pending in the monitor, which cannot grant access. Questions,
secret inputs and connected-tool forms retain the same deferral/decline behavior.
Duplicate/stale requests do not replay an answer. A turn that completes
after a deferred interaction remains a blocked failed outcome, including after
saved-thread reconciliation, rather than claiming the requested work succeeded.

Connected-tool authorization URLs are fingerprinted and stripped from saved
results, sources and diagnostics, including later recovery and followups. No
pending authorization URL appears in the monitor or persona report. Actual source
and scoped artifact links remain in durable evidence for explanation through chat.
Citation control markers are removed from display, and missing URLs are never
fabricated. Original user messages and frozen persona history are not rewritten.

**Stop** affects conversation generation. The core retains explicit task cancellation
for lifecycle control, but the monitor exposes no Codex actions. Interruption only
becomes confirmed after the actual turn outcome; an acknowledged interrupt without
confirmation becomes uncertain after five seconds and cannot be blindly resubmitted.
A normal owner `delegate_task(task_id)` for an uncertain task reads only its saved
thread after reporting finishes; it never submits a new turn or silently retries work.
Earlier completed effects remain saved. Closing Pebble interrupts its managed
children; it does not promise work continues after quit.
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
are never resent. Recovery reads actual
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
for ordinary latest failed-reply **Retry** after repair without restarting or executing work again.
Ordinary chat and explicit fresh contexts can run during task
execution; reporting uses the current accepted context/state. The report gets
stored evidence, has no active work or memory tools, and publishes through the
normal validated reply/state transaction. A failed explanation exposes the ordinary latest-reply **Retry**, which uses the
stored result without rerunning Codex. Failed, uncertain
or interrupted execution can also be explained accurately without pretending
that the requested work completed. Each report freezes its outcome revision and
evidence. A later reconciled outcome receives its own report; earlier queued or
streaming reports cannot publish over that newer outcome. Repeated callbacks do
not report the same revision again. Ordinary bubble Retry preserves latest/idle/membership eligibility and retries
only the saved explanation, including after a fresh context. Older failed
explanations remain saved; a later user continuation can reuse the task’s saved
thread once reporting is no longer pending.

The origin room and original membership interval must still be eligible when
publication occurs. Removing/rejoining a character does not revive the old
interval. An invalid destination retains its saved result and receives no public
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
deferred questions, declined manual approvals, automatic approval review lifecycles,
managed writable/read-only workspaces, same-thread followups, cancellation, restart,
report retry, continued chat/context changes, shared/explicit group assignments
and membership privacy. A [followup fixture](../crates/pebble-core/tests/delegation_followup.rs)
checks original/continuation/objective payloads through both native APIs, core
followups, reloads and frozen scope. Opt-in probes use isolated synthetic roots; ordinary
checks never access personal calendars or notes.

The Approve for me increment adds deterministic policy, review lifecycle and
managed workspace coverage with validation logs under ignored
`build/validation/approve-for-me-2026-10-12/`. The notification payloads are based
on the installed Codex 0.160.1 experimental app-server schema; these review
notifications are marked unstable by that schema.

Earlier local evidence is kept under ignored
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
