#!/usr/bin/env bash
# Run `pylspt-dev calls` on jarvis_cli.py: wire tree + context graph, as JSON.
# usage: ./run-jarvis.sh [out-dir] [extra calls flags, e.g. --ids --merged]
set -euo pipefail

cd "$(dirname "$0")"

PROJECT=/Users/yared/Documents/Programing/personal/pythonJaRvis.github.io/Jarvis/tool/Jarvis
FILE=$PROJECT/processing/extProcessor.py
ENTRY=ExtProcessor.analyze_localfunction

OUT=${1:-.}
shift || true
mkdir -p "$OUT"

cargo build --offline --quiet --bin pylspt-dev
BIN=./target/debug/pylspt-dev

"$BIN" calls "$PROJECT" "$FILE" --entry "$ENTRY" --wire  --pretty "$@" -o "$OUT/cli-wire.json"
"$BIN" calls "$PROJECT" "$FILE" --entry "$ENTRY" --graph --pretty "$@" -o "$OUT/cli-graph.json"

echo "wrote $OUT/cli-wire.json and $OUT/cli-graph.json"
