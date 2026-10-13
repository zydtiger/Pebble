set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/prepare-ui-checks.sh
mkdir -p build/validation/reply-markdown
pebble_ui_sources=()
for pebble_source in macos/Pebble/*.swift; do
  if [[ "$pebble_source" != */PebbleApp.swift ]]; then
    pebble_ui_sources+=("$pebble_source")
  fi
done
xcrun swiftc "${pebble_swiftmath_flags[@]}" -swift-version 6 -strict-concurrency=complete -warnings-as-errors -parse-as-library \
  -target arm64-apple-macosx14.0 \
  -I macos/Generated -Xcc -fmodule-map-file=macos/Generated/PebbleCoreFFI.modulemap \
  macos/Generated/PebbleCore.swift "${pebble_ui_sources[@]}" macos/Tests/MarkdownChecks.swift macos/Tests/MathChecks.swift \
  target/debug/libpebble_ffi.a -liconv -o build/markdown-checks
pebble_markdown_root=$(mktemp -d "$PWD/build/markdown-data.XXXXXX")
trap 'rm -rf "$pebble_markdown_root"' EXIT
PEBBLE_DATA_DIR="$pebble_markdown_root" build/markdown-checks
