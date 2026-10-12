set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p build/validation/image-window
xcrun swiftc -swift-version 6 -strict-concurrency=complete -warnings-as-errors -parse-as-library \
  -target arm64-apple-macosx14.0 macos/Pebble/Theme.swift macos/Pebble/ImagePreviewWindow.swift \
  macos/Tests/ImagePreviewChecks.swift -o build/image-preview-checks
pebble_preview_root=$(mktemp -d "$PWD/build/image-preview-data.XXXXXX")
trap 'rm -rf "$pebble_preview_root"' EXIT
PEBBLE_PREVIEW_DATA="$pebble_preview_root" build/image-preview-checks
