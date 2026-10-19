#!/bin/sh
set -eu
project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
"$project_root/scripts/cargo.sh" build --release --workspace --locked
export STORYFOLD_ROOT="$project_root"
exec "$project_root/target/release/storyfold-desktop"
