set -euo pipefail
cd "$(dirname "$0")/.."
bash scripts/build-rust.sh
pebble_image_sources=()
while IFS= read -r pebble_image_source; do pebble_image_sources+=("$pebble_image_source"); done < <(rg --files macos/Pebble -g '*.swift' -g '!PebbleApp.swift')
mkdir -p build/ImageChecks.app/Contents/MacOS
cat > build/ImageChecks.app/Contents/Info.plist <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.pebble.image-checks</string>
<key>CFBundleName</key><string>ImageChecks</string>
<key>CFBundleExecutable</key><string>ImageChecks</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
xcrun swiftc -swift-version 6 -strict-concurrency=complete -warnings-as-errors \
  -target arm64-apple-macosx14.0 -parse-as-library -I macos/Generated \
  -Xcc -fmodule-map-file=macos/Generated/PebbleCoreFFI.modulemap \
  macos/Generated/PebbleCore.swift "${pebble_image_sources[@]}" macos/Tests/ImageChecks.swift \
  target/debug/libpebble_ffi.a -liconv -o build/ImageChecks.app/Contents/MacOS/ImageChecks
pebble_image_root=$(mktemp -d "$PWD/build/image-ui-data.XXXXXX")
pebble_image_evidence=${PEBBLE_IMAGE_EVIDENCE:-"$PWD/build/previews"}
mkdir -p "$pebble_image_evidence"
echo "Native picker fixture: $pebble_image_root/synthetic-circle.png"
: > "$pebble_image_evidence/native-stdout.log"
: > "$pebble_image_evidence/native-stderr.log"
open -W -n --env "PEBBLE_DATA_DIR=$pebble_image_root" --env "PEBBLE_IMAGE_EVIDENCE=$pebble_image_evidence" \
  --stdout "$pebble_image_evidence/native-stdout.log" --stderr "$pebble_image_evidence/native-stderr.log" build/ImageChecks.app
cat "$pebble_image_evidence/native-stdout.log" "$pebble_image_evidence/native-stderr.log"
rg -q '^Native image checks passed:' "$pebble_image_evidence/native-stdout.log"
