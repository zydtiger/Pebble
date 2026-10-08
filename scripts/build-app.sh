set -euo pipefail
cd "$(dirname "$0")/.."
# Generate bindings before XcodeGen discovers Swift source files.
bash scripts/build-rust.sh
xcodegen generate --spec macos/project.yml
xcodebuild -project macos/Pebble.xcodeproj -scheme Pebble \
  -configuration Debug -derivedDataPath build/xcode build
mkdir -p build
ditto build/xcode/Build/Products/Debug/Pebble.app build/Pebble.app
