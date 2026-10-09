# Pebble

Pebble is a macOS character-chat app. Read `README.md` and `docs/architecture.md`
before changing the application. The current milestone is v0.0.5, leading to the
private-chat release v0.1.0.

## Ownership and scope

- `crates/pebble-core` owns configuration, persona identity, SQLite, model history,
  and generation lifecycle. Keep it independent of Swift and UniFFI.
- `crates/pebble-ffi` owns typed binding conversion and foreign listeners.
- `macos/Pebble` owns SwiftUI/AppKit presentation and platform glue only.
- `macos/Generated`, `target`, and `build` are generated; edit their sources.
- `docs/architecture.md` owns runtime boundaries; `docs/roadmap.md` owns scope.
- v0.0.4 supports multiple independent personas, per-persona serialized queues,
  concurrent private-room streams, and character creation. Swift keeps per-room
  presentation state; Rust owns execution order and model history. Do not add
  groups, compaction, compression, settings, retry, or task delegation in this
  milestone. v0.0.5 adds Responses with whole native output-item replay; preserve
  Chat Completions and the independent persona queues.

## Development and validation

Use `just build` to produce the local app and `just run` to open it. The hook
configuration is the mechanical validation contract; `just check` runs it.
Use `just check-rust` for core/FFI changes, `just check-bridge` for binding and
event-lifecycle changes, and `just build` for presentation changes. Review a
rendered app window after visual changes. Documentation-only edits need the
file-level hooks and `git diff --check`, not application builds.

Keep authored files and tests small and readable. Add targeted tests for
behavior, ownership, persistence, and cancellation; do not test generated code
by reproducing its implementation. Never commit runtime data or credentials.

## Git and distribution

The initial scaffold is on `main`. Subsequent work uses `<prefix>/<short-topic>`
branches with prefixes from the commit categories below. Keep any worktrees
beside this checkout. Preserve unrelated local changes.

Commit subjects use `prefix: imperative summary`, without scopes or a trailing
period. Allowed prefixes: `feat` (behavior), `fix` (correctness), `docs`
(documentation), `refactor` (structure), `test` (validation), `build` (tooling or
packaging), and `chore` (maintenance). Commits, merges, publication, tags, and
releases each require user authorization. No remote or public release is
configured. The local development app is not a signed distribution release.

Repository instructions and docs must remain self-contained. Do not depend on
personal vaults, global skills, or a particular developer's home directory.
