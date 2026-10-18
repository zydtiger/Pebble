default:
    @just --list

build:
    python3 scripts/prune-build-cache.py
    bash scripts/build-app.sh

run: build
    open build/Pebble.app

check:
    git ls-files --cached --others --exclude-standard -z | xargs -0 prek run --hook-stage pre-commit --files
    git ls-files --cached --others --exclude-standard -z | xargs -0 prek run --hook-stage pre-push --files

check-rust:
    prek run cargo-clippy --all-files --hook-stage pre-push
    prek run cargo-test --all-files --hook-stage pre-push

check-bridge:
    bash scripts/check-bridge.sh

preview:
    bash scripts/render-preview.sh

icon:
    open -a "Icon Composer" macos/Pebble/Resources/Pebble.icon

preview-icon:
    bash scripts/preview-icon.sh
