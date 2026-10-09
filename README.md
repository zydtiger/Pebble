# Pebble

A small space for familiar voices.

Pebble is a native Mac app for persistent characters, conversation, and
roleplay. Its name connects an everyday object with *pebbling*: sharing small
things as gestures of care.

**v0.0.1 is a working foundation and interface preview.** It loads one starter
character, opens a private room, and streams a clearly labeled, scripted reply
from Rust into SwiftUI. It does not call a model or save preview messages.

## Run it

Requirements: Apple Silicon Mac, Xcode with Swift 6.3 or later and Icon Composer,
Rust 1.98.1 (including rustfmt and Clippy), XcodeGen 2.46 or later, and `just`. Validation also uses `prek`
0.4.14 or later. These tools are developer prerequisites, not app dependencies.
The app's deployment target is macOS 14; it is currently exercised on the
development host, not a matrix of older macOS releases.

```sh
just run
```

This builds and opens `build/Pebble.app`. It is ad-hoc signed for local use;
no Developer ID, App Store account, remote repository, or release is required.
You can reopen that app directly without rebuilding.

The build generates a local Xcode project at `macos/Pebble.xcodeproj`. Open it
after the first build if you want to iterate in Xcode. Generated bindings and
project files are ignored; their source of truth is Rust plus `macos/project.yml`.

## App icon

The editable [Pebble.icon](macos/Pebble/Resources/Pebble.icon) document contains
two vector pebbles with Liquid Glass materials. Run `just icon` to edit it in
Icon Composer, or `just preview-icon` to export appearance and small-size
previews into `build/icon-preview/`. `just build` compiles the icon into the app.
See [the icon design notes](docs/app-icon.md) for artwork and material settings.

## Try the first stage

1. Open Mira's private space and choose **Say hello**, or type a thought and
   press **Enter**. **Command–Enter** or **Shift–Enter** inserts a new line. The composer grows
   with your text, then scrolls at its maximum height; the send button stays at
   the lower right.
2. Stop a reply while it streams. This cancels the worker in Rust.
3. Open the character information panel to read SOUL and MEMORY.
4. Use **Open character folder**, edit the files, and choose **Reload Character
   Files** from the app menu or sidebar. Character and room IDs remain stable.
5. Quit and reopen the app. Character files and private-room identity survive;
   preview bubbles intentionally disappear.

The interface is an early light-theme design with a starter character. Groups,
live inference, durable chat messages, automated memory, and work delegation
belong to later milestones. See [the roadmap](docs/roadmap.md).

## Local data

On first launch, Rust seeds an empty data root with a character and editable
files. Existing files are never overwritten by the seed operation.

```text
~/Library/Application Support/Pebble/
├── config.toml
├── personas/<persona-uuid>/
│   ├── config.toml
│   ├── SOUL.md
│   └── MEMORY.md
└── data/
    └── app.sqlite        # personas + conversations only at v0.0.1
```

SQLite may also create its own WAL and shared-memory sidecars. Keep this data
outside the source checkout. `PEBBLE_DATA_DIR` selects an isolated root for
development and tests; an existing, incomplete root reports an error instead
of silently reseeding it.

The app config optionally supplies `[defaults.chat]`. A persona can provide its
own `[chat]` table, replacing the whole connection. Connections require `model`,
`base_url`, and `api` (`chat_completions` or `responses`) together. `api_key_env`
is an optional reference, never the key itself. v0.0.1 validates these files but
does not perform inference or read credentials. An omitted connection means
no model is configured.

## Architecture

```text
SwiftUI / AppKit presentation
          │ typed records + event listener
          ▼
UniFFI Swift bindings → one Rust static library
          │
          ▼
pebble-core
  configuration · personas · private-room IDs · SQLite · preview lifecycle
```

Swift handles rendering, input, temporary preview bubbles, and platform paths.
Rust owns authoritative identities, configuration validation, database access,
and the simulated stream. The core has no dependency on Swift or UniFFI.
Read [the architecture](docs/architecture.md) for the boundary and lifecycle.

## Validate

```sh
just check          # file checks, Rust lint/tests, Swift 6 FFI integration
just build          # native application compile and local signing
just preview        # render actual SwiftUI views into build/previews/
```

`just check-rust` and `just check-bridge` run narrower gates. The hook definitions
in `.pre-commit-config.yaml` own the exact validation commands. Install `prek`
on the development machine and run `prek install` in a clone to activate its
pre-commit, pre-push, and commit-message hooks. Hooks are not installed globally.

Tests use isolated data roots; UI-check data stays under the ignored `build/`
directory for inspection. The Rust tests cover restart
identity, schema scope, file edits, connection inheritance, invalid files,
Unicode event order, cancellation, and callback lifetime. The Swift executable
checks the real generated FFI and presentation adapter under complete Swift 6
concurrency checking. The renderer captures this app's own content without
capturing the desktop or other applications.

Nothing is committed, published, or released automatically by these commands.
