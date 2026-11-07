#!/bin/bash
# usage: consumer.sh install | upgrade
. /scripts/common.sh
set +e

case "$1" in
  install)
    step "versions"; brew --version
    step "brew tap via dumb HTTP from S3"
    brew tap "$TAP" "$TAP_URL"; echo "exit=$?"
    brew tap-info --json "$TAP" | grep -o '"trusted":[^,}]*'
    step "install by short name (expect tap-trust refusal)"
    brew install ophello; echo "exit=$?"
    step "install fully-qualified (expect bottle pour from S3)"
    brew install "$TAP/ophello"; echo "exit=$?"
    ophello
    brew info "$TAP/ophello" | head -5
    ;;
  upgrade)
    step "brew update (dumb HTTP fetch)"
    brew update; echo "exit=$?"
    brew outdated
    step "brew upgrade"
    brew upgrade; echo "exit=$?"
    ophello
    step "trust state after upgrade"
    brew trust
    ;;
esac
