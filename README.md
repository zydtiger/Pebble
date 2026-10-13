# Pebble

Pebble is a native macOS app for chatting and roleplaying with persistent AI
characters, individually or together in a group.

## Features

- **Private and group chats.** Talk to one character or create a group with any
  combination. Characters can decide when to join in; `@Name` requests a reply.
- **Personalities and memory.** Customize each character's personality and
  memories, shared across their private and group conversations. Start a fresh
  context while keeping saved conversations.
- **Your choice of models.** Connect compatible local or hosted models, with
  app defaults and individual character settings.
- **Formatted replies.** Read bold and italic text, code blocks, and native inline
  or display LaTeX math.
- **Image sharing.** Paste, drop or select JPEG/PNG images, and chat about them
  with a vision-capable model.
- **Tasks through Codex.** Let characters delegate research and other work to
  local Codex, then bring the results back into chat.
- **Reply notifications.** Receive macOS notifications when the chat window is
  unfocused; click one to return to its conversation.
- **Local history.** Conversations and character files are saved on your Mac.

## Install

Pebble currently runs from a source build. You need:

- An Apple Silicon Mac running macOS 14 or later.
- Full Xcode with Swift 6.3 or later and Icon Composer. Open Xcode once to finish
  its setup.
- Rust installed with [rustup](https://rustup.rs). The repository selects the
  required Rust toolchain automatically.
- XcodeGen 2.46 or later and `just`.

Install the build tools with Homebrew, then clone and run Pebble:

```sh
brew install just xcodegen
git clone https://github.com/zydtiger/Pebble.git
cd Pebble
just run
```

The first build fetches the pinned SwiftMath package and bundles its math fonts.
This builds and opens `build/Pebble.app`. You can reopen that app directly
without rebuilding.

### Connect your models

1. Open **App defaults** using the sidebar gear. Enter your chat model's name,
   API base URL, API type, context and output limits, and API key if required.
   Without a model connection, Pebble runs a local demo.
2. Use **Character settings** to customize a character or choose a different
   model for them.
3. Configure **Participation model** to let group members decide when to reply
   automatically. Manual mentions work without it.
4. To delegate work, install and sign in to Codex locally, then enable it in
   **Codex tasks**.

Press **Enter** to send; **Shift–Enter** or **Command–Enter** inserts a newline.
The first send requests notification permission.

[Developer documentation](docs/development.md) · [MIT License](LICENSE)
