# Pebble

Pebble is a macOS character-chat app. Read `README.md` and `docs/architecture.md`
before changing the application. The current baseline is the v0.3.0 local
private/group-chat app with selective participation, bounded room scheduling, editable future-only membership, manual mentions, structured
replies, durable state, and native contract controls. The current image-input
increment adds durable static JPEG/PNG attachments to this baseline. The
version-unassigned Codex delegation increment adds local task execution, persona-mediated
questions/permission limits, durable evidence and queued private/group reporting.

## Ownership and scope

- `crates/pebble-core` owns configuration, persona identity, SQLite, model history,
  and generation lifecycle. Keep it independent of Swift and UniFFI.
- `crates/pebble-ffi` owns typed binding conversion and foreign listeners.
- `macos/Pebble` owns SwiftUI/AppKit presentation and platform glue only.
- `macos/Generated`, `target`, and `build` are generated; edit their sources.
- `docs/architecture.md` owns runtime boundaries; `docs/roadmap.md` owns scope.
- The private-chat foundation supports independent persona queues, Chat Completions/Responses native
  replay, bounded raw/zstd history, context budgets, fresh contexts, native
  app/persona settings, validated responses, durable state/schema controls,
  and explicit latest-reply retry/recovery. Swift owns
  presentation; Rust owns validation, files, context, history, and execution.
  The completed v0.1.3–v0.2.0 sequence provides group rooms, visibility, response
  classification, bounded reply scheduling, and membership/recovery as defined
  in docs/roadmap.md. The completed v0.2.1–v0.3.0 sequence adds persona-scoped native memory updates and explicit asynchronous memory consolidation with durable context markers. Rust owns tool effects, recovery and maintenance; Swift only presents them. Rust also owns immutable image assets, visibility, model/classifier image inputs, and frozen replay; Swift presents pick/paste/drop and image views. See docs/images.md. Rust also owns local stdio Codex tasks, scope, durable identity/recovery, native
  interaction deferral and original-room result delivery; Swift presents the
  compact task monitor/settings. See docs/delegation.md for capability and permission limits.
  Future changes need scoped authorization. Automatic compaction, image
  generation, deletion, import/export and cloud sync remain deferred.

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
releases each require user authorization. The public source repository is
https://github.com/zydtiger/Pebble. The local development app is not a signed
distribution release.

Repository instructions and docs must remain self-contained. Do not depend on
personal vaults, global skills, or a particular developer's home directory.
