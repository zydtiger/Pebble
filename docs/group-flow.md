# Ordered group conversation flow

A canonical user post creates one durable round, member observations, mentions,
classifier decisions and reply slots. No assistant bubble or persona queue
reservation exists for a future slot. Required user mentions run in accepted
mention order. Optional members follow in least-recently-accepted public reply
order, with membership ordinal/ID ties; network response latency cannot reorder
them. Every eligible member may receive one initial admission. Required replaces
that member's optional slot.

Only the ready room head creates its ordinary persona-global call and placeholder.
Other rooms/private calls remain independent. A pending optional decision is
re-evaluated after relevant accepted predecessors or actual persona knowledge/
state changes. If it joined a busy persona queue, it rechecks at activation after
the predecessor completes. Classifier generations prevent stale results from
winning. Classification never writes state, native items or event coverage.

After initial slots drain, accepted public reply text can prompt another
classification wave, excluding the immediately previous speaker. At most one
candidate per wave is admitted in stable order. A whole round spends at most two
bot followup admissions, in addition to its initial member set. Admission spends
the budget atomically; failed/cancelled calls do not refund it. Format repair
uses the same admission. Bot `@` text is ordinary text and never forces a member.
Whole-group fallback instead allows at most one acknowledgment and ends the
round; [participation](participation.md) defines its exact two-pass protocol.
Its frozen per-call `reply_request` carries `purpose: group_acknowledgment` and
instructions to give only a brief acknowledgment or clarification, without an
ordinary substantive answer or a new discussion. This does not rewrite the
session prefix or ordinary reply requests. Format repair keeps that same frozen
request; eligible explicit Retry preserves the purpose, including after a fresh
context, without reopening the chain. The model still produces the reply under
the normal validated response schema; brevity is instructed, not a separate
semantic guarantee enforced by that schema.

New same-room user input cancels old optional/automatic work, including admitted
calls, while preserving unfinished required user mentions. Those old slots keep
their original root, room, member interval and observation cutoff. The next room
round waits for protected older work to drain, without occupying future persona
queues. An old required call can therefore execute after an actual newer private
call: it retains that accepted native prefix and latest state. Cutoffs restrict
new observations, not already accepted knowledge.

For later slots, accepted earlier-slot public replies in the same round are
explicitly eligible even above the original cutoff. Later unrelated user events
stay excluded unless already represented by actual accepted persona history.
Exact event links retain coverage holes. For example, U1 requests B then A; U2
arrives while B runs. A includes B's accepted reply, excludes newly injected U2,
and a later call sees U2 exactly once. If A actually completed a private call P
that consumed U2 first, delayed A retains P's entire native prefix/state and does
not inject U2 again. Both protocols exercise these paired cases.

Admission, Stop/new-input invalidation and settings publication share the persona
acceptance gate. Final completion checks active slot/round/member interval in the
same transaction as reply/state/native output/sent event/public fanout. If Stop
wins, a late network result has no acceptance effects. Room Stop reaches every
unfinished round and classifier/call in that room, including protected old @
slots, and preserves shared personas' private/other-room work. Individual Stop
waiting cancels an unadmitted slot; bubble Cancel stops its admitted call.

Observers remain alive until their durable round and dynamically admitted calls
settle. Waiting slots and intent/permission phases are presentation statuses,
never chat-history messages. Fresh context, state/schema and connection saves
require no unfinished still-admittable slot/decision/call in the affected scope;
ordinary private sends can still run first. Startup marks unfinished work
Interrupted and never resumes classifiers or chains. Latest persona-wide Retry
may run after an ended round if the member interval remains valid and persona
is idle; it never reopens a round or refunds automatic budget.
