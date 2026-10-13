set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/prepare-ui-checks.sh
mkdir -p build/validation/persona-avatars
pebble_ui_sources=()
for pebble_source in macos/Pebble/*.swift; do
  if [[ "$pebble_source" != */PebbleApp.swift ]]; then
    pebble_ui_sources+=("$pebble_source")
  fi
done
xcrun swiftc "${pebble_swiftmath_flags[@]}" -swift-version 6 -strict-concurrency=complete -warnings-as-errors -parse-as-library \
  -target arm64-apple-macosx14.0 \
  -I macos/Generated -Xcc -fmodule-map-file=macos/Generated/PebbleCoreFFI.modulemap \
  macos/Generated/PebbleCore.swift "${pebble_ui_sources[@]}" macos/Tests/AvatarChecks.swift \
  target/debug/libpebble_ffi.a -liconv -o build/avatar-checks
pebble_avatar_root=$(mktemp -d "$PWD/build/avatar-data.XXXXXX")
trap 'rm -rf "$pebble_avatar_root"' EXIT
PEBBLE_DATA_DIR="$pebble_avatar_root" build/avatar-checks
