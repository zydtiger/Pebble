set -euo pipefail
cd "$(dirname "$0")/.."
bash scripts/build-rust.sh
mkdir -p build/previews
xcrun swiftc -swift-version 6 -strict-concurrency=complete -parse-as-library \
  -target arm64-apple-macosx14.0 \
  -I macos/Generated -Xcc -fmodule-map-file=macos/Generated/PebbleCoreFFI.modulemap \
  macos/Generated/PebbleCore.swift macos/Pebble/Delegation.swift macos/Pebble/ChatModel.swift macos/Pebble/RoomState.swift macos/Pebble/ImageInput.swift macos/Pebble/SettingsTarget.swift \
  macos/Pebble/Theme.swift macos/Pebble/MessageEditor.swift \
  macos/Pebble/ImageAttachments.swift macos/Pebble/ContentView.swift macos/Pebble/TaskMonitor.swift macos/Pebble/CodexSheet.swift macos/Pebble/ParticipationPanel.swift macos/Pebble/ClassifierSheet.swift macos/Pebble/SettingsSheet.swift macos/Pebble/RequestParametersEditor.swift macos/Pebble/StateSheet.swift macos/Pebble/NewCharacterSheet.swift macos/Pebble/GroupSheet.swift macos/Pebble/GroupMembersSheet.swift macos/Tests/RenderChecks.swift macos/Tests/DelegationChecks.swift macos/Tests/ComposerChecks.swift macos/Tests/GroupRetryChecks.swift macos/Tests/ClassifierChecks.swift macos/Tests/MembershipChecks.swift \
  target/debug/libpebble_ffi.a -liconv -o build/render-preview
pebble_preview_root=$(mktemp -d "$PWD/build/render-data.XXXXXX")
PEBBLE_DATA_DIR="$pebble_preview_root" build/render-preview
