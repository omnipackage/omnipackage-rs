set -euo pipefail
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 HOMEBREW_NO_ANALYTICS=1 HOMEBREW_NO_ENV_HINTS=1 HOMEBREW_NO_INSTALL_FROM_API_HINT=1
PUB=https://repositories-test.omnipackage.org/homebrew-spike
TAP_URL="$PUB/homebrew-tap.git"
TAP=omnitest/spike
step() { printf '\n===== %s =====\n' "$*"; }
