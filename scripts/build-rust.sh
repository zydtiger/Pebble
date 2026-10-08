set -euo pipefail
cd "$(dirname "$0")/.."
pebble_profile=debug
pebble_cargo_profile=dev
if [ "${CONFIGURATION:-Debug}" = Release ]; then
  pebble_profile=release
  pebble_cargo_profile=release
fi
cargo build -p pebble-ffi --locked --profile "$pebble_cargo_profile"
cargo run -p pebble-ffi --features bindgen --bin pebble-bindgen --locked -- \
  generate --library "target/$pebble_profile/libpebble_ffi.a" \
  --language swift --out-dir macos/Generated
