#!/usr/bin/env bash
# Dev: open okru-tui via `cargo run` on the workspace DB (./data/okru.db).
# It connects to okru-backend on 127.0.0.1:ipcPort (read from dist/config.toml,
# the same config the local backend uses) to notify changes and show bot activity.
#
#   ./tui.sh                 # data/okru.db
#   ./tui.sh --db other.db   # any other SQLite file
set -euo pipefail

cd "$(dirname "$0")"

export OKRU_CONFIG="${OKRU_CONFIG:-$PWD/dist/config.toml}"

exec cargo run --quiet -p okru-tui -- "$@"
