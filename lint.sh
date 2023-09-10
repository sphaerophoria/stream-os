#!/usr/bin/env bash

set -ex

cargo +nightly fmt --check
cargo +nightly clippy -- -Dwarnings
cargo +nightly test
