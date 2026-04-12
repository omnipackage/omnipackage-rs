#!/usr/bin/env bash

set -xEeuo pipefail

if cargo --version; then
    exit 0
fi

curl -O https://static.rust-lang.org/dist/rust-1.94.1-x86_64-unknown-linux-gnu.tar.gz
tar -xzf rust-*.tar.gz
cd rust-*/
./install.sh --prefix=/usr
cd ..
rm -rf rust-*
