#!/usr/bin/env bash

set -xEeuo pipefail

if cargo --version; then
    exit 0
fi

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
rm -rf ~/.rustup/tmp ~/.rustup/toolchains/*/share/doc
ln -s $HOME/.cargo/bin/* /usr/bin/
