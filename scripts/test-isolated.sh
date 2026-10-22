#!/usr/bin/env bash
# Unit and TUI tests write preferences and send DSP commands; isolate both.
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/micstudio-tests.XXXXXXXX")
trap 'rm -rf -- "$test_root"' EXIT
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export HOME="$test_root"
export XDG_CONFIG_HOME="$test_root/.config"
export PIPEWIRE_REMOTE="micstudio-tests-no-server"
export PULSE_SERVER="unix:$test_root/no-pulse-server"
cd -- "$project_root"
cargo test --locked --offline -j 2 --lib --test dsp_tests --test tui_layout -- --test-threads=1
