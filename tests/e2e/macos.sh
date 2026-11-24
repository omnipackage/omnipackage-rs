#!/bin/sh
# Manual end-to-end test of the `macos` (Homebrew) target. Run on a macOS host with podman or docker:
#   cargo build && sh tests/e2e/macos.sh
# Cross-compiles tests/fixtures/macos_project in a container, publishes it, then installs/upgrades it with brew like an end user.
set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${OMNIPACKAGE_BIN:-$ROOT/target/debug/omnipackage}
REPOSITORY=${REPOSITORY:-R2 test}
ENV_FILE=${ENV_FILE:-$ROOT/.env}
PKG=omnipackage-macos-hello
TAP=omnipackage/$PKG
WORK=$(mktemp -d)
PROJECT=$WORK/project

step() { printf '\n===== %s =====\n' "$*"; }
fail() { echo "macos e2e: FAIL: $*" >&2; exit 1; }
gone() { case "$1" in http*) ! curl -fsI "$1" >/dev/null 2>&1 ;; *) [ ! -e "$1" ] ;; esac; }
cleanup() {
  brew uninstall --force "$TAP/$PKG" >/dev/null 2>&1 || true
  brew untap "$TAP" >/dev/null 2>&1 || true
}

[ "$(uname -s)" = Darwin ] || fail "run this on macOS"
command -v brew >/dev/null || fail "brew not found"
command -v podman >/dev/null || command -v docker >/dev/null || fail "podman or docker not found"
[ -x "$BIN" ] || fail "no omnipackage binary at $BIN (cargo build first)"
cleanup
trap cleanup EXIT

cp -R "$ROOT/tests/fixtures/macos_project" "$PROJECT"
set_version() { printf 'package main\n\nconst version = "%s"\n' "$1" > "$PROJECT/version.go"; }
omni() { "$BIN" "$@" "$PROJECT" --env-file "$ENV_FILE"; }

PAGE=$(omni info --show-install-page-url --repository "$REPOSITORY")
BASE=${PAGE%/install.html}
[ -n "$BASE" ] || BASE=/tmp/omnipackage-macos-repo
TAP_URL=$BASE/macos/tap.git
echo "repository: $REPOSITORY, base: $BASE"

step "release 1.0.0"
set_version 1.0.0
omni release --distros macos --repository "$REPOSITORY" --build-dir "$WORK/build-1"
ls "$WORK"/build-1/*/output/

step "build 1.0.1"
set_version 1.0.1
omni build --distros macos --build-dir "$WORK/build-2"

step "consumer: tap and install 1.0.0"
brew tap "$TAP" "$TAP_URL"
brew install "$TAP/$PKG" 2>&1 | tee "$WORK/install.log"
! grep -q "Pouring" "$WORK/install.log" || fail "1.0.0 was poured from a bottle, expected the prebuilt binary"
! xattr -p com.apple.quarantine "$(brew --prefix)/bin/$PKG" >/dev/null 2>&1 || fail "binary is quarantined"
codesign -v "$(brew --prefix)/bin/$PKG" || fail "binary has no valid ad-hoc signature"
"$(brew --prefix)/bin/$PKG" | grep -q "$PKG 1.0.0" || fail "1.0.0 binary output"

step "publish 1.0.1, consumer: brew update && brew upgrade"
omni publish --distros macos --repository "$REPOSITORY" --build-dir "$WORK/build-2"
gone "$BASE/macos/formulae/1.0.0/$PKG-1.0.0-macos.tar.gz" || fail "retention did not remove the 1.0.0 archive"
brew update
brew upgrade "$TAP/$PKG" 2>&1 | tee "$WORK/upgrade.log"
! grep -q "Pouring" "$WORK/upgrade.log" || fail "1.0.1 was poured from a bottle, expected the prebuilt binary"
"$(brew --prefix)/bin/$PKG" | grep -q "$PKG 1.0.1" || fail "1.0.1 binary output"
brew test "$TAP/$PKG"

step "install.sh"
cleanup
case "$BASE" in
  http*) curl -fsSL "$BASE/install.sh" | sh -s -- -y ;;
  *) sh "$BASE/install.sh" -y ;;
esac
"$(brew --prefix)/bin/$PKG" | grep -q "$PKG 1.0.1" || fail "install.sh did not install 1.0.1"

step "report"
ls "$WORK"/build-2/*/output/*.tar.gz
brew info "$TAP/$PKG" | head -3
echo "install page: $PAGE"
echo "macos e2e: PASS (workdir $WORK)"
echo "remove published files: ruby $ROOT/scripts/clear_bucket.rb repositories-test macos-e2e"
