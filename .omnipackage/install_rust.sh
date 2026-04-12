#!/usr/bin/env bash

set -xEeuo pipefail

if cargo --version; then
    exit 0
fi

export RUSTUP_HOME=/opt/rust/rustup
export CARGO_HOME=/opt/rust/cargo
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
mv /opt/rust/cargo/bin/* /usr/bin/

rm -rf ~/.rustup/tmp ~/.rustup/toolchains/*/share/doc
