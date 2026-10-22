#!/usr/bin/env bash
# Unit and TUI tests write preferences and send DSP commands; isolate both.
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/micstudio-tests.XXXXXXXX")
trap 'rm -rf -- "$test_root"' EXIT
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
cd -- "$project_root"
env CARGO_HOME="$cargo_home" HOME="$test_root" \
    XDG_CONFIG_HOME="$test_root/.config" PIPEWIRE_REMOTE="micstudio-tests-no-server" \
    PULSE_SERVER="unix:$test_root/no-pulse-server" \
    cargo test --locked --offline -j 2 --lib --test dsp_tests --test tui_layout -- --test-threads=1
