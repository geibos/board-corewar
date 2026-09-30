#!/bin/sh
# cw.wasm: the engine and the assembler for a web page (see src/lib.rs).
# Needs the wasm32-unknown-unknown target: rustup target add wasm32-unknown-unknown
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
cp ../target/wasm32-unknown-unknown/release/cw_wasm.wasm cw.wasm
ls -l cw.wasm
