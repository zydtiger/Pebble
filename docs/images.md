# Image input

Private and group messages accept text with up to four images, or images alone.
Use the composer's **+** button, paste an image, or drop image files on the native
text editor. Draft thumbnails can be removed or opened before sending. Transcript
images open in a larger view. Draft text, mentions and images belong to their
room; switching rooms or an import/acceptance error retains them. Acceptance
clears only the unchanged submitted draft revision. A newer edit retains its
complete draft. Imports in progress finish into their original room.

Files must be static JPEG or PNG. macOS TIFF clipboard representations are
explicitly converted to PNG before the same Rust validation. HEIC files,
animations, remote URLs, arbitrary documents, generated images, attachment
removal from saved messages and retention/deletion policies are outside this
increment. The unconfigured demo remains labeled as a demo; it has no image
understanding model.

## Assets and limits

Rust validates content rather than trusting a filename or MIME declaration.
Each import has a new UUID under `data/assets/<UUID>/`, containing the unchanged
source, an orientation-corrected fixed `model-v1.jpg` or `model-v1.png`, and a
256-pixel thumbnail. Normalization never enlarges a small image. Chat,
classification and memory maintenance use the same saved model rendition.
Original files remain available for full-resolution viewing. Removing an image
from a draft removes that selection; it does not delete durable assets.

| Boundary | Limit |
| --- | --- |
| Images per accepted message | 4, with explicit order |
| Original bytes per image | 12 MiB |
| Decoded image | 20 million pixels; each dimension at most 20,000 |
| Decoder allocation limit | 160 MiB |
| Model rendition | Longest edge 768 pixels, at most 2 MiB |
| Final JSON request | 32 MiB; accumulated rendition/base64 allocation is preflighted |
| Decoded native history | Independent existing 8 MiB bound |
| Vision estimate | 2048 estimated tokens per image occurrence, plus text/metadata accounting |

The fixed 768-pixel boundary reflects bounded validation of the selected local
services. A 1536-pixel image exposed a llama.cpp noncausal image-batch assertion
in the selected chat service; its server configuration was not changed. Four
768-pixel images were read in order through both chat protocols. The selected
classifier measured 609 total prompt tokens for one 768-pixel synthetic image;
the chat service measured 290, and 1068 for four images. Four-image classifier
requests counted about 2346 prompt tokens and changed the boolean answer when
only the final picture changed from red to blue against three blank pictures.
This small classifier was less reliable on repeated-color order references. These are measurements
of these services, not universal image-token formulas. A fixed 2048 reserve per
image is conservative for these tested inputs. Text and metadata continue to
count separately, and arbitrary providers may still reject a request. Small
text/details can be lost at the current model resolution; the full original is
kept for viewing.

Files and containing directories are synced before publishing SQLite metadata.
Schema v12 adds `image_assets` and `message_images`; it preserves existing IDs,
text records and native history bytes. Message attachments, observations, rounds
and queued calls commit in one transaction. A failed import or interrupted
prepublication write can leave an identifiable unreferenced UUID directory or
imported draft asset. No automatic cleanup or broad deletion is performed.

## Frozen input and visibility

New multimodal user inputs contain typed `pebble_image_v1` parts whose references
record asset UUID, exact rendition, SHA-256, bytes, rendition dimensions, MIME
and fixed `high` detail. Envelope metadata and preceding text parts associate images with
their message and participant. Plain text and provider output are never parsed
to discover assets. The HTTP adapter verifies the saved rendition and temporarily
materializes a data URL into native Chat Completions `image_url` or Responses
`input_image` parts. SQLite and per-persona histories never duplicate base64.
See the [native vision input specification](https://developers.openai.com/api/docs/guides/images-vision).

Retry, restart, format repair and memory-tool continuation replay the same
rendition bytes and parameters, without reencoding. Missing or corrupt required
renditions fail visibly. New prefixes explain image attribution; preserved old
prefixes and opaque native reasoning/output stay unchanged.

All selection happens through existing delivered-event, membership and cutoff
rules before attachment loading. Private images reach only their persona.
Public images fan out as references to eligible members, including listeners.
New and rejoining members observe future publications only. Image-only messages
retain their normal user identity, round and participation lifecycle.

Initial participation, reassessment, whole-group intent and real-user permission
include the selected messages' actual images. Repeated references to one message
are attached once in a classifier request. Optional old events can be removed as
complete text/image units to fit the classifier budget. Required root, trigger,
same-round predecessors and permission inputs cannot silently lose images;
mandatory overflow fails before HTTP. Classifier reads do not consume events or
change native chat history, accepted state or coverage.

Explicit consolidation receives outgoing native image inputs and eligible silent
observations. It can preserve useful visual facts in textual MEMORY. The new
context excludes prior images at the published cutoff; transcript originals
remain visible. Textual memory of a picture does not restore access to its
original details.

## Validation

`cargo test -p pebble-core --test image_inputs` exercises both HTTP adapters,
ordered real pixels, immutable replay through failure/restart/Retry, and silent
image consolidation followed by a fresh context. Rust unit tests cover asset,
EXIF, migration, visibility, limits and all classifier phase budgets, including
four images under the seeded 16384-token classifier window. `bash
scripts/check-images.sh` exercises the actual native picker, TIFF pasteboard
conversion, file-drop handlers, draft/revision behavior and rendered image views
using an isolated runtime root. The script opens an isolated test app and pauses
for selection of its printed synthetic PNG path in the system Open panel;
remaining lifecycle and rendering checks proceed automatically. `just check-rust`, `just check-bridge` and
`just build` remain the required implementation gates.

Opt-in synthetic visual controls use the real adapters: set `PEBBLE_LIVE_API_ROOT`
and `PEBBLE_LIVE_MODEL`, then run the ignored
`opt_in_selected_chat_protocols_consume_changed_pixels` test in `image_inputs`.
The classifier's ignored `opt_in_selected_classifier_consumes_changed_image_pixels`
unit test takes `PEBBLE_CLASSIFIER_API_ROOT` and `PEBBLE_CLASSIFIER_MODEL`.
Identical text with changed red/blue pixels must produce changed correct answers;
HTTP success alone is insufficient evidence.

New visible image inputs also include `original_size` with the immutable asset's
original width/height, separately from the at-most-768-pixel vision rendition.
This lets image editing prefer original dimensions when the user gives no size.
The attribution text labels both sizes explicitly. Legacy references omit this
optional metadata and remain readable; new inputs can enrich it from existing
asset rows without updating those rows or rewriting frozen history.
