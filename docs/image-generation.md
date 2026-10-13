# Image generation

Pebble exposes native `generate_image` and `edit_image` tools to configured persona contexts.
All three arguments are required; extra keys, omitted dimensions, non-integer
sizes and mixed native calls are rejected before execution:

```json
{
  "prompt": {
    "subject": "One pebble beside a cup of tea",
    "style": "watercolor",
    "composition": null,
    "lighting": null,
    "background": null,
    "details": null,
    "constraints": ["no text"]
  },
  "backend": "studio",
  "size": {"width": 1024, "height": 768}
}
```

`prompt` is a fixed JSON object. `subject` must be a nonblank string; optional
`style`, `composition`, `lighting`, `background` and `details` are descriptive
strings, and optional `constraints` is an array of strings. Values are freeform.
The strict provider schema requires every field, using `null` for unused optional
fields. Runtime validation also accepts omitted optional fields. Unknown or
duplicate keys and incorrect types are rejected at every object level; new
tool calls cannot use a plain string prompt.

Personas receive instructions to describe a self-contained image, preserve the
user's specific visual requirements, use only applicable fields and avoid
gratuitous inventions. Backend, model, path and pixel size stay outside `prompt`.
Rust omits unused/null fields and serializes the validated object directly to
one compact JSON string, preserving supplied text without a prose renderer or
another model rewrite. The serialized payload, including keys and JSON escaping,
must fit 16384 UTF-8 bytes. Studio's 8000-character bound also applies to that
serialized payload. Studio receives it as the request's `prompt` **string**;
ChatGPT receives the same string inside its existing image-task envelope.

A call creates one image. Backend choices are restricted to configured backends;
no URL, model, GPU, path, seed or inference override belongs in tool arguments.
The shared tool instructions tell personas to choose `studio` by default and
`chatgpt` only when the user explicitly requests ChatGPT. If Studio is unavailable
or fails, explain the limitation without switching backends automatically. This
is a model-selection instruction; `backend` remains required and the runtime
executes the explicitly supplied, validated backend. The current tool description
applies to subsequent requests; existing frozen system prefixes remain unchanged.
Newly created/fresh contexts acquire this capability; schema 14 preserves older
prefixes and native output without rewriting them. A persona needs function
calling and vision to request an image and inspect its result. Classifiers only
observe published images, using the existing image rendition and budgets.

## Editing a published image

`edit_image` accepts exactly one source asset ID and produces one new immutable
asset. The original remains unchanged. Required arguments are `image_id`,
`prompt`, `backend` and explicit `size.width`/`size.height`:

```json
{
  "image_id": "the-published-asset-uuid",
  "prompt": {
    "instruction": "Change the cup from red to green",
    "preserve": ["the cat", "rainy window", "cup shape"],
    "style": null,
    "composition": null,
    "lighting": null,
    "background": null,
    "details": null,
    "constraints": ["keep the existing label"]
  },
  "backend": "studio",
  "size": {"width": 1024, "height": 768}
}
```

`instruction` must be a nonblank string; `preserve` and `constraints` are
optional string arrays, and the remaining optional fields are strings. The
strict provider schema requires all fields with nullable optional values;
runtime validation also accepts omitted optional fields. Unknown/duplicate
keys and wrong types are rejected. Rust omits nulls and serializes the object
directly to compact JSON under the same byte/Studio character limits as creation.
Studio is the default; ChatGPT requires an explicit user request. Backend failure
never causes a fallback.

The source must be published in the current room and exposed as an actual image
part in the persona's frozen request, with a matching durable persona event link
inside that session boundary. Drafts, other rooms, group images from before
joining, and future messages cannot supply edit sources. User uploads and
previously published generated or edited images use the same check, including
successive edits. If no source is visible or the user's reference is ambiguous,
the persona must ask the user to identify or attach the intended image before
calling `edit_image`; it must never guess an ID or silently choose a source.
No arbitrary paths or URLs are accepted. The accepted operation
freezes the original source filename and metadata, never the 768-pixel vision
rendition, together with the operation kind, prompt and settings. Legacy records
without these fields default to generation; no history is rewritten.

Visible image metadata distinguishes `original_size` from the normalized vision
rendition's `width`/`height`. When the user does not specify an output size, the
persona should prefer the source's original dimensions and still supply explicit
`size.width`/`size.height`. If those dimensions are unavailable, outside the tool's
256–2048 range, or unsupported by the backend profile, it must explain the limit
and ask for a supported size instead of silently resizing or changing backends.
Older references without `original_size` still decode and replay unchanged;
new visible inputs can obtain original dimensions from their immutable asset rows,
without rewriting saved references or native history.

Studio validates the selected profile's `image-edit` mode, input count, byte and
pixel limits and existing output-size/settings constraints before uploading.
It uploads the original JPEG/PNG once via multipart `POST /api/inputs`, saves the
returned input ID, then submits `POST /api/generations` with `mode=image-edit`,
`input_image_ids=[input_id]` and `count=1`. Even a prior Studio output is uploaded
as the local immutable source. Upload and submission are bounded and never
retried. Saved upload/run identities and dispatch uncertainty survive restart;
recovery never reuploads or redispatches, and only reconciles a saved run. An
interrupted upload or lost submit without a run ID stays uncertain and is
explained in chat.

ChatGPT copies the original into the dedicated operation workspace on initial
dispatch and attaches it as typed `localImage` input alongside edit instructions
and the exact prompt JSON. The controller may only use its built-in image tool;
shell and unrelated tools remain forbidden. Output comes only from typed
`imageGeneration` events; dimensions remain best effort. Recovery reads the
saved thread and never stages or starts another turn. Advertised image-generation
capability alone does not prove live editing support.

Editing shares generation's canonical-user-request deduplication, monitor,
original-room report queue, retry and cancellation/membership rules. One user
trigger can claim only one image operation across both tools. The saved result
includes operation kind/source identity and actual output pixels; report retry
cannot edit again. Publication attaches only the new asset to the persona reply
and atomically fans it out to group members. The monitor uses the typed operation kind to label creation as **Generate image**
and editing as **Edit image**. There is no separate editor or additional settings UI.

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
frozen serialized prompt/size/settings/executable, dispatch uncertainty, Studio run/artifact
or Codex thread/turn/item identities, immutable local asset ID, actual dimensions,
and separate execution/publication states. Native call/result replay commits in
the same transaction as the operation, before dispatch. One canonical user
request claims one shared image operation; repeated tool calls and reply retries
reuse that claim.

The stored operation's prompt remains a string containing the exact backend
payload. Existing operations with a plain prose string continue to load and
reconcile their saved remote identities unchanged; startup does not parse or
convert that string. No schema migration or rewrite of frozen native replay is
needed. This read compatibility does not admit new plain-string tool calls or
resubmit interrupted operations.

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

Batches, variants, generation after quit, automatic duplicate
submission, backend fallback and asset deletion remain outside this increment.

## Validation

`cargo test -p pebble-core --lib image_generation` covers both native protocols,
strict structured arguments and serialized byte bounds, matching JSON-string
payloads for both backends, legacy-prompt restart recovery, Studio's serialized
character limit, edit schema/null serialization, current-room frozen source visibility,
original-source preservation, successive edits, shared generate/edit deduplication,
multipart upload and dispatch uncertainty without resubmission, profile
constraints/default omission, HTTP and typed app-server
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

Earlier generation-only validation completed one live Studio generation at 256 × 256,
with the returned default steps, guidance and model revision retained. Two
ChatGPT probes returned typed `failed` status with no image and no typed failure
reason. The second probe's isolated controller log identified an HTTP 404 from
the configured Responses-only proxy, which does not proxy the image endpoint.
Those historical failures show that login and advertised image capability alone
do not prove endpoint access. Neither failed operation was automatically retried,
and no user provider configuration changed.

Live edit validation on 2026-10-13 used the actual configured persona/chat model
and copied configuration in an isolated data root, without copying or changing
real app history. Three sequential native `edit_image` operations completed and
published: Studio requested a rainy-to-sunny background edit; Studio then requested
a blue-cup edit using that published output without reattaching it; an explicit
ChatGPT request then edited the second output toward a gray cat. Both Studio
outputs were 1024 × 1024. ChatGPT returned 1254 × 1254 for requested 1024 × 1024;
Pebble retained the actual image and explained the mismatch. Native tool arguments,
source/output IDs and hashes, Studio input/run/artifact IDs and Codex
thread/turn/item IDs were retained. Each source matched the previous published
output, the initial original remained unchanged, and no operation was retried or
switched to another backend. This verifies live edit execution/publication in
that configuration; exact ChatGPT output dimensions remain best effort.
