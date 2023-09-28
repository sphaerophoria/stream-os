#!/usr/bin/env bash

set -ex

cargo +nightly fmt --check
cargo +nightly clippy -- -Dwarnings
NOGRAPHIC=1 cargo +nightly test
