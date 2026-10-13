# Image generation

Pebble exposes one native `generate_image` tool to configured persona contexts.
All three arguments are required; extra keys, omitted dimensions, non-integer
sizes and mixed native calls are rejected before execution:

```json
{"prompt":"One watercolor pebble beside tea","backend":"studio","size":{"width":1024,"height":768}}
```

A call creates one image. Backend choices are restricted to configured backends;
no URL, model, GPU, path, seed or inference override belongs in tool arguments.
Newly created/fresh contexts acquire this capability; schema 14 preserves older
prefixes and native output without rewriting them. A persona needs function
calling and vision to request an image and inspect its result. Classifiers only
observe published images, using the existing image rendition and budgets.

## Configuration

Use the native **Image generation** sheet or app `config.toml`. These settings
are independent of chat, classifier and delegated-task model settings:

```toml
[image_generation.studio]
base_url = "https://api.example.com"
model_id = "configured-registration-id"
gpu_id = "configured-gpu-id"
# steps = 40
# guidance = 1.0

[image_generation.chatgpt]
# model = "your-local-controller-model"
# effort = "medium"
```

Table presence enables a backend. Remove its table to disable it. Empty ChatGPT
model/effort inherit local Codex configuration. The executable comes from
`[codex].executable`; delegation enablement, model, effort and workspace policy
do not override the image controller. ChatGPT requires a local ChatGPT login and
provider `imageGeneration` capability. Before exposing ChatGPT to a persona,
Pebble probes local login/capability without creating a thread or turn. Successful
checks are cached for the current configuration during this process; failed checks
are retried on a later request so external login can take effect. Saving image or
Codex settings clears the cache. Each generation validates again. The controller is distinct from the image
backend model, whose identity is not supplied by the typed result.

Studio maps `model_id` to `registration_id`, `gpu_id` to `gpu_uuid`, and dimensions
to `width`/`height`; it submits `count=1`. Absent steps/guidance are omitted so
server defaults apply. Returned effective parameters (including steps/guidance,
model revision/profile and GPU) are saved on acceptance and polling, and frozen
into result evidence. Before submission, Rust reads model readiness and
`/api/profiles`, checks its dimension multiple, step bounds and fixed guidance.
The supported dimension range is 256–2048 pixels. Studio prompts are limited to
8000 characters. Use a trusted reachable Studio service; this adapter adds no
authentication protocol.

Studio dimensions are strict. An unexpected size retains the imported asset and
failure evidence, but cannot publish an image as a successful result. ChatGPT
size is best effort: the installed app-server contract has no typed exact-size
control. Pebble sends width/height and framing intent, preserves the generated
image, records actual dimensions and tells the persona to explain any mismatch.
It never regenerates or changes backends to obtain a different size.

## Execution and publication

Schema 14 stores operation identity, owner/original room and membership interval,
frozen prompt/size/settings/executable, dispatch uncertainty, Studio run/artifact
or Codex thread/turn/item identities, immutable local asset ID, actual dimensions,
and separate execution/publication states. Native call/result replay commits in
the same transaction as the operation, before dispatch. One canonical user
request claims one shared image operation; repeated tool calls and reply retries
reuse that claim.

The tool returns promptly. Generation runs outside the persona queue, with its
own 30-minute bound; Studio queue/model loading does not spend the chat operation's
180-second budget. The existing top-right monitor includes image queue, loading,
execution and cancellation. Terminal work disappears from that monitor and the
persona explains results or limits in ordinary chat.

Studio uses direct HTTP submit → poll → bounded PNG download. It constructs
artifact routes from saved identities at the configured origin, follows no
redirects and imports through existing immutable asset validation. It never
resumes Studio's shared queue. Paused/interrupted runs require service recovery.
ChatGPT uses one managed stdio app-server child with a dedicated writable image
workspace for tool-owned output; unrelated execution and delegation remain
forbidden. It matches typed `imageGeneration` events by thread/turn/item. Typed
image failures retain usage-limit details when supplied, stop the controller and
enter ordinary persona reporting without automatic retry. It saves completed
image bytes immediately, separately from controller turn completion. It never parses prose
or scans recently changed files for results. Image frames have a separate bounded
18 MiB limit and eight-event queue; ordinary delegation retains its original
2 MiB frame limit.

A stored result later enters the original persona queue using current accepted
context/state. Its frozen report envelope includes actual model image parts,
requested/actual dimensions and outcome evidence. The validated reply, state,
message attachment, author event and group fanout commit together. Other members
and classifiers receive those public pixels; the author retains native image
observation without a duplicate public receipt. Membership is checked against
the original interval, so leaving and rejoining never revives an old image.
Report snapshots remain frozen; a reconciled outcome supersedes an older pending
report rather than changing its stored model input.

## Stop, quit and recovery

Conversation **Stop** stays available while an image or its queued publication is
pending. It invalidates every unpublished image initiated in that room and
requests backend cancellation. Member removal does the same for that owner.
Late bytes may be retained as evidence but cannot publish. Cancellation
acknowledgement alone never proves remote work stopped: Studio receives one
cancel request and a bounded outcome check; Codex receives an interrupt with a
short confirmation grace. Unconfirmed termination stays uncertain.

Quitting stops managed children and requests cancellation of Studio work. Remote
work may outlive the client. Startup only reconciles saved run/thread identities
with read-only GET/thread-read operations and may poll an already submitted
Studio run. It never submits a replacement, resumes a controller turn or resumes
the global Studio queue. A lost Studio submit response has no confirmed run ID
and cannot be reliably reconciled because Studio has no client idempotency key.
It stays uncertain and is never automatically resent. A new generation requires
a new explicit user image request.

Download failures preserve the remote identity for retrieval on startup. Existing
local assets keep their IDs during reconciliation. Failed persona publication
uses ordinary latest-reply **Retry**, with tools disabled and saved image input;
it cannot rerun generation. Unpublished stopped work is ineligible for Retry.

Reference editing, batches, variants, generation after quit, automatic duplicate
submission, backend fallback and asset deletion remain outside this increment.

## Validation

`cargo test -p pebble-core --lib image_generation` covers both native protocols,
strict arguments, profile constraints/default omission, HTTP and typed app-server
fixtures, lost submission, identity-based recovery, pixels in persona input,
queue release, acknowledgment/report retries, atomic group fanout, Stop and
original membership invalidation. `just check-rust`, `just check-bridge` and
`just build` are required integration gates. Native bridge checks cover typed
settings and queued-image Stop/monitor visibility. Render the focused settings
and loading monitor with `PEBBLE_RENDER_IMAGES_ONLY=1 bash scripts/render-preview.sh`.

An ignored `live_image_generation_probe` test uses an isolated temporary data
root. Select `PEBBLE_IMAGE_PROBE_BACKEND=studio` and explicitly supply
`PEBBLE_IMAGE_PROBE_STUDIO_URL`, `PEBBLE_IMAGE_PROBE_MODEL_ID` and
`PEBBLE_IMAGE_PROBE_GPU_ID`, or select `chatgpt` with explicit
`PEBBLE_IMAGE_PROBE_CONTROLLER`. Optional `PEBBLE_IMAGE_PROBE_EXECUTABLE` selects
Codex. Set an absolute `PEBBLE_IMAGE_PROBE_OUTPUT_DIR` to retain original PNG
and concise identity/dimension evidence. Run only when a real generation is
authorized; fixture success does not prove live GPU, quota or model access.

The implementation validation completed one live Studio generation at 256 × 256,
with the returned default steps, guidance and model revision retained. Two
ChatGPT probes returned typed `failed` status with no image and no typed failure
reason. The second probe's isolated controller log identified an HTTP 404 from
the configured Responses-only proxy, which does not proxy the image endpoint.
ChatGPT live generation therefore remains unverified in this environment; login
and the advertised image capability do not prove endpoint access. Neither failed
operation was automatically retried, and no user provider configuration changed.
