#!/bin/sh
set -eu
project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
for toolchain in "$project_root/data/toolchain" "$project_root/catalyst-solo/boilerplate/data/toolchain"; do
    if [ -x "$toolchain/cargo/bin/cargo" ]; then
        export RUSTUP_HOME="$toolchain/rustup"
        export CARGO_HOME="$toolchain/cargo"
        export PATH="$CARGO_HOME/bin:$PATH"
        break
    fi
done
cd "$project_root"
exec cargo "$@"
