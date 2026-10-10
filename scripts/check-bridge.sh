set -euo pipefail
cd "$(dirname "$0")/.."
bash scripts/build-rust.sh
mkdir -p build
xcrun swiftc -swift-version 6 -strict-concurrency=complete -warnings-as-errors \
  -target arm64-apple-macosx14.0 -parse-as-library -I macos/Generated \
  -Xcc -fmodule-map-file=macos/Generated/PebbleCoreFFI.modulemap \
  macos/Generated/PebbleCore.swift macos/Pebble/ChatModel.swift macos/Pebble/RoomState.swift macos/Pebble/SettingsTarget.swift \
  macos/Tests/BridgeChecks.swift macos/Tests/GroupRetryChecks.swift target/debug/libpebble_ffi.a \
  -liconv -o build/bridge-checks
pebble_test_root=$(mktemp -d "$PWD/build/bridge-data.XXXXXX")
PEBBLE_DATA_DIR="$pebble_test_root" build/bridge-checks
