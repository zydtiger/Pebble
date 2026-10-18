# Development

See the [README](../README.md#install) for prerequisites and the source build.
The generated Xcode project is `macos/Pebble.xcodeproj`; open it after the first
build to iterate in Xcode. Generated bindings and project files are ignored;
Rust and `macos/project.yml` are their source of truth. Builds are ad-hoc signed
for local use, not signed or notarized distribution releases.

## Validate

### Rust build cache

Dev and test profiles use `line-tables-only` debug information: backtraces keep
source locations, while debugger variable/type inspection is limited. Incremental
compilation remains enabled.

Before building, `just build` (also `just run`) trims this checkout's
`target/debug` cache to a default budget of 20 GiB. It removes the oldest artifact
groups and incremental directories first, stopping once below budget. Related
fingerprints, dependencies and build-script outputs are removed together; final
top-level libraries/executables and unrecognized files are retained. Release,
Xcode outputs and other worktrees are outside its scope.

This is approximate LRU: ordering uses the latest filesystem access/modification
time in each group, without reading file contents. Filesystems may delay or disable
access-time updates, and other readers may refresh them. It is not an exact Cargo
usage log. Accounting uses allocated bytes and deduplicates hardlinks; APFS shared
extents can make actual reclaimed free space differ.

Cleanup acquires Cargo's Debug directory locks without waiting and skips cleanup
if a build is active. It runs before the build, so a build can exceed the budget
until the next invocation. Retained files can also prevent reaching the budget;
the script reports this rather than falling back to a full `cargo clean`.
Direct Cargo/Xcode/script builds do not trigger cleanup. No background job runs.

Preview or manually change the budget for one cleanup:

```sh
python3 scripts/prune-build-cache.py --dry-run
python3 scripts/prune-build-cache.py --max-gib 30
python3 -B scripts/test-build-cache.py
```

Python 3 is required for the `just build` preflight. The default budget lives in
`scripts/prune-build-cache.py`; tune it after measuring normal build/test usage.

### Checks

```sh
just check          # file checks, Rust lint/tests, Swift 6 FFI integration
just build          # native application compile and local signing
just preview        # render actual SwiftUI views into build/previews/
bash scripts/check-markdown.sh # reply Markdown/math checks and native rendering
bash scripts/check-avatars.sh # targeted avatar reload and native rendering checks
```

`just check-rust` and `just check-bridge` run narrower gates. The hook definitions
in `.pre-commit-config.yaml` own the exact validation commands. Install `prek`
on the development machine and run `prek install` in a clone to activate its
pre-commit, pre-push, and commit-message hooks. Hooks are not installed globally.

[GitHub Actions](../.github/workflows/ci.yml) runs on `macos-latest` for pull
requests, pushes to `main`, and manual dispatch. It uses `just check` for the
pre-commit and pre-push stages (file checks, actionlint, Rust formatting/lint/tests,
and Swift FFI integration), then runs the manual application-build hook. It
does not run opt-in live endpoint tests or require model credentials.
Rust uses the repository's pinned toolchain. Dependabot checks Actions weekly
and Cargo, Rust toolchain, and hook updates monthly. The workflow's `prek-version`
minor range and Homebrew build tools are maintained manually.

Tests use isolated data roots; UI-check data stays under the ignored `build/`
directory for inspection. The Rust tests cover restart
identity, schema scope, file edits, connection inheritance, invalid files,
Unicode event order, cancellation, callback lifetime, migration, frozen request
prefixes, multiple native output items and opaque replay, fragmented SSE, model failure states, event
links, concurrent persona queues, queued cancellation, transactional rollback,
and abrupt-process-exit recovery. The Swift executable
checks the real generated FFI and presentation adapter under complete Swift 6
concurrency checking. The renderer captures this app's own content without
capturing the desktop or other applications.
The full-view UI check scripts build the native app before linking its pinned
SwiftMath package and copying its font resource bundle. Markdown/math checks
retain screenshots under `build/validation/reply-markdown/` and use an isolated,
temporary demo data root. They cover delimiter parsing, source fallback, copy,
TextKit wrapping and equation layout at different window widths.

Avatar checks retain screenshots under `build/validation/persona-avatars/` and
remove their isolated temporary persona files when finished.

Live endpoint validation is opt-in; ordinary tests only use local deterministic
servers. The following test seeds synthetic character files in an isolated
temporary root, saves settings, completes two turns across restart, and checks early cancellation plus explicit retry.
Replace the example API root and model with your deployed service:

```sh
PEBBLE_LIVE_API_ROOT=https://api.example.com/v1 \
PEBBLE_LIVE_MODEL=your-model \
cargo test -p pebble-core --test live_endpoint opt_in_live_two_turns_restart_and_cancel -- --ignored --nocapture
```

For Responses, use the same environment variables and replace the test name
with `opt_in_live_responses_two_turns_restart_and_cancel`. For llama.cpp Responses, also set `PEBBLE_LIVE_RESPONSES_SCHEMA_FORMAT=response_format`; standard Responses uses `text.format` by default. See [provider contracts](providers.md).

`PEBBLE_LIVE_API_KEY` optionally supplies a test credential. The test does not
read the app's actual character files or chat history and prints no reasoning
content or credentials.

Nothing is committed, published, or released automatically by these commands.

## App icon

The editable [Pebble.icon](../macos/Pebble/Resources/Pebble.icon) contains the
Liquid Glass artwork. Run `just icon` to open Icon Composer, or
`just preview-icon` to export previews to `build/icon-preview/`.
See [icon design and materials](app-icon.md).

## Implementation documentation

- [Architecture and runtime boundaries](architecture.md)
- [Local data and connection files](local-data.md)
- [Settings and recovery](settings.md)
- [Provider protocols](providers.md), [history](history.md), and [context budgets](context.md)
- [Groups](groups.md), [participation](participation.md), and [reply scheduling](group-flow.md)
- [Character state](state.md) and [memory](memory.md)
- [Image input](images.md) and [Codex delegation](delegation.md)
- [Milestones and remaining scope](roadmap.md)
