#!/usr/bin/env bash

set -xEeuo pipefail

if cargo --version; then
    exit 0
fi

export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/usr/local/cargo
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
rm -rf ~/.rustup/tmp ~/.rustup/toolchains/*/share/doc
ln -s /usr/local/cargo/bin/* /usr/bin/
