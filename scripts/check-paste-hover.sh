set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/prepare-ui-checks.sh
pebble_paste_sources=()
while IFS= read -r pebble_paste_source; do pebble_paste_sources+=("$pebble_paste_source"); done < <(rg --files macos/Pebble -g '*.swift' -g '!PebbleApp.swift')
mkdir -p build/PasteHoverChecks.app/Contents/MacOS
ditto "$pebble_swiftmath_products/SwiftMath_SwiftMath.bundle" build/PasteHoverChecks.app/Contents/Resources/SwiftMath_SwiftMath.bundle
cat > build/PasteHoverChecks.app/Contents/Info.plist <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.pebble.paste-hover-checks</string>
<key>CFBundleName</key><string>PasteHoverChecks</string>
<key>CFBundleExecutable</key><string>PasteHoverChecks</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
xcrun swiftc "${pebble_swiftmath_flags[@]}" -swift-version 6 -strict-concurrency=complete -warnings-as-errors \
  -target arm64-apple-macosx14.0 -parse-as-library -I macos/Generated \
  -Xcc -fmodule-map-file=macos/Generated/PebbleCoreFFI.modulemap \
  macos/Generated/PebbleCore.swift "${pebble_paste_sources[@]}" macos/Tests/PasteHoverChecks.swift \
  target/debug/libpebble_ffi.a -liconv -o build/PasteHoverChecks.app/Contents/MacOS/PasteHoverChecks
pebble_paste_root=$(mktemp -d "$PWD/build/paste-ui-data.XXXXXX")
pebble_paste_evidence=${PEBBLE_PASTE_EVIDENCE:-"$PWD/build/validation/paste-hover-2026-10-11"}
mkdir -p "$pebble_paste_evidence"
: > "$pebble_paste_evidence/native-stdout.log"
: > "$pebble_paste_evidence/native-stderr.log"
open -W -n --env "PEBBLE_DATA_DIR=$pebble_paste_root" --env "PEBBLE_PASTE_EVIDENCE=$pebble_paste_evidence" \
  --stdout "$pebble_paste_evidence/native-stdout.log" --stderr "$pebble_paste_evidence/native-stderr.log" build/PasteHoverChecks.app
cat "$pebble_paste_evidence/native-stdout.log" "$pebble_paste_evidence/native-stderr.log"
rg -q '^Native Paste checks passed:' "$pebble_paste_evidence/native-stdout.log"
