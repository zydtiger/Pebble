set -euo pipefail
cd "$(dirname "$0")/.."
icon_tool="$(xcode-select -p)/../Applications/Icon Composer.app/Contents/Executables/ictool"
icon_source="macos/Pebble/Resources/Pebble.icon"
mkdir -p build/icon-preview
for appearance in Default Dark ClearLight ClearDark TintedLight TintedDark; do
    "$icon_tool" "$icon_source" --export-image \
        --output-file "build/icon-preview/$appearance.png" \
        --platform macOS --rendition "$appearance" --width 512 --height 512 --scale 1
done
for size in 32 64 256; do
    "$icon_tool" "$icon_source" --export-image \
        --output-file "build/icon-preview/Default-$size.png" \
        --platform macOS --rendition Default --width "$size" --height "$size" --scale 1
done
