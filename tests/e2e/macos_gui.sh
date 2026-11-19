#!/bin/sh
# Manual end-to-end test of the `macos` target in cask mode. Run on a macOS host from anywhere:
#   cargo build && sh tests/e2e/macos_gui.sh
# Builds tests/fixtures/macos_gui_project into a .dmg, publishes it as a cask, then installs/upgrades it with brew like an end user.
set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${OMNIPACKAGE_BIN:-$ROOT/target/debug/omnipackage}
REPOSITORY=${REPOSITORY:-R2 test}
ENV_FILE=${ENV_FILE:-$ROOT/.env}
PKG=omnipackage-macos-hello-gui
TAP=omnipackage/$PKG
APP="/Applications/Omnipackage Hello.app"
EXE="$APP/Contents/MacOS/$PKG"
WORK=$(mktemp -d)
PROJECT=$WORK/project

step() { printf '\n===== %s =====\n' "$*"; }
fail() { echo "macos gui e2e: FAIL: $*" >&2; exit 1; }
cleanup() {
  pkill -f "$APP/" >/dev/null 2>&1 || true
  brew uninstall --cask --force "$TAP/$PKG" >/dev/null 2>&1 || true
  brew untap "$TAP" >/dev/null 2>&1 || true
}
gone() { case "$1" in http*) ! curl -fsI "$1" >/dev/null 2>&1 ;; *) [ ! -e "$1" ] ;; esac; }
quarantined() { if xattr -p com.apple.quarantine "$APP" >/dev/null 2>&1; then echo yes; else echo no; fi; }
check_version() { "$EXE" --version | grep -q "$PKG $1" || fail "expected $PKG $1, got: $("$EXE" --version)"; }
check_launch() {
  open "$APP"
  sleep 3
  pgrep -f "$APP/" >/dev/null || fail "app did not launch (Gatekeeper?)"
  pkill -f "$APP/"
}

[ "$(uname -s)" = Darwin ] || fail "run this on macOS"
command -v brew >/dev/null || fail "brew not found"
command -v swiftc >/dev/null || fail "swiftc not found (xcode-select --install)"
[ -x "$BIN" ] || fail "no omnipackage binary at $BIN (cargo build first)"
[ ! -e "$APP" ] || brew list --cask "$TAP/$PKG" >/dev/null 2>&1 || fail "$APP exists and is not from $TAP; remove it first"
cleanup
trap cleanup EXIT

cp -R "$ROOT/tests/fixtures/macos_gui_project" "$PROJECT"
set_version() { printf '%s\n' "$1" > "$PROJECT/version.txt"; }
omni() { "$BIN" "$@" "$PROJECT" --env-file "$ENV_FILE"; }

PAGE=$(omni info --show-install-page-url --repository "$REPOSITORY")
BASE=${PAGE%/install.html}
[ -n "$BASE" ] || BASE=/tmp/omnipackage-macos-gui-repo
TAP_URL=$BASE/macos/tap.git
echo "repository: $REPOSITORY, base: $BASE"

step "release 1.0.0"
set_version 1.0.0
omni release --distros macos --repository "$REPOSITORY" --build-dir "$WORK/build"
ls "$WORK"/build/*/output/

step "consumer: tap and install --cask 1.0.0"
brew tap "$TAP" "$TAP_URL"
brew install --cask "$TAP/$PKG"
[ -d "$APP" ] || fail "$APP not installed"
Q1=$(quarantined)
xattr -dr com.apple.quarantine "$APP"
check_version 1.0.0
check_launch

step "release 1.0.1, consumer: brew update && brew upgrade --cask"
set_version 1.0.1
omni release --distros macos --repository "$REPOSITORY" --build-dir "$WORK/build"
gone "$BASE/macos/casks/1.0.0/$PKG-1.0.0.dmg" || fail "retention did not remove the 1.0.0 dmg"
brew update
brew upgrade --cask "$TAP/$PKG"
Q2=$(quarantined)
xattr -dr com.apple.quarantine "$APP"
check_version 1.0.1
check_launch

step "install.sh (runs the xattr step itself)"
cleanup
case "$BASE" in
  http*) curl -fsSL "$BASE/install.sh" | sh -s -- -y ;;
  *) sh "$BASE/install.sh" -y ;;
esac
[ "$(quarantined)" = no ] || fail "install.sh left the app quarantined"
check_version 1.0.1
check_launch

step "report"
ls "$WORK"/build/*/output/
brew info --cask "$TAP/$PKG" | head -3
echo "quarantined after install: $Q1, after upgrade: $Q2"
echo "install page: $PAGE"
echo "macos gui e2e: PASS (workdir $WORK)"
echo "remove published files: ruby $ROOT/scripts/clear_bucket.rb repositories-test macos-gui-e2e"
