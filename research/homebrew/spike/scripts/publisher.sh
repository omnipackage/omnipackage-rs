#!/bin/bash
# usage: publisher.sh init | release <version>
. /scripts/common.sh

export RCLONE_CONFIG_R2_TYPE=s3 RCLONE_CONFIG_R2_PROVIDER=Cloudflare RCLONE_CONFIG_R2_NO_CHECK_BUCKET=true
export RCLONE_CONFIG_R2_ACCESS_KEY_ID="$CLOUDFLARE_R2_ACCESS_KEY_ID" RCLONE_CONFIG_R2_SECRET_ACCESS_KEY="$CLOUDFLARE_R2_SECRET_ACCESS_KEY" RCLONE_CONFIG_R2_ENDPOINT="$CLOUDFLARE_R2_ENDPOINT"
DST=r2:repositories-test/homebrew-spike
W=/tmp/w
BARE=$W/tap.git

make_src() {
  local v=$1 d=$W/src/ophello-$1
  mkdir -p "$d"
  printf '#include <stdio.h>\nint main(void){puts("ophello %s");return 0;}\n' "$v" > "$d/main.c"
  printf 'PREFIX ?= /usr/local\nall:\n\tcc -O2 -o ophello main.c\ninstall: all\n\tinstall -Dm755 ophello $(PREFIX)/bin/ophello\n' > "$d/Makefile"
  tar -C "$W/src" -czf "$W/src/ophello-$v.tar.gz" "ophello-$v"
  rclone copyto "$W/src/ophello-$v.tar.gz" "$DST/src/ophello-$v.tar.gz"
  sha256sum "$W/src/ophello-$v.tar.gz" | cut -d' ' -f1
}

write_formula() {
  local v=$1 sha=$2 dir=$3
  mkdir -p "$dir/Formula"
  cat > "$dir/Formula/ophello.rb" <<EOF
class Ophello < Formula
  desc "omnipackage homebrew spike"
  homepage "https://omnipackage.org"
  url "$PUB/src/ophello-$v.tar.gz"
  sha256 "$sha"
  license "MIT"

  def install
    system "make", "install", "PREFIX=#{prefix}"
  end

  test do
    assert_match "ophello $v", shell_output("#{bin}/ophello")
  end
end
EOF
  cat > "$dir/Formula/opnet.rb" <<EOF
class Opnet < Formula
  desc "probe: is network available during install"
  homepage "https://omnipackage.org"
  url "$PUB/src/ophello-1.0.tar.gz"
  version "1.0"
  sha256 "$(sha256sum "$W/src/ophello-1.0.tar.gz" | cut -d' ' -f1)"

  def install
    system "curl", "-fsS", "-o", "/dev/null", "https://example.com"
    bin.mkpath
    (bin/"opnet").write("#!/bin/sh\necho network-ok\n")
  end
end
EOF
}

publish_repo() {
  local tapdir=$1
  git -C "$tapdir" push -q origin HEAD:main
  git -C "$BARE" update-server-info
  rclone sync "$BARE" "$DST/homebrew-tap.git"
}

bottle_and_publish() {
  local tapdir v=$1
  tapdir=$(brew --repo "$TAP")
  step "build bottle $v (brew install --build-bottle)"
  brew install --build-bottle "$TAP/ophello"
  mkdir -p "$W/bottles/$v" && cd "$W/bottles/$v"
  step "brew bottle"
  brew bottle --json --root-url="$PUB/bottles" "$TAP/ophello"
  ls -la
  cat ./*.json
  local local_name remote_name
  local_name=$(ls ./*.bottle*.tar.gz | head -1)
  remote_name=$(grep -o '"filename": *"[^"]*"' ./*.json | head -1 | sed 's/.*"filename": *"//; s/"$//')
  echo "local=$local_name remote=$remote_name"
  rclone copyto "$local_name" "$DST/bottles/$remote_name"
  step "merge bottle block into formula"
  brew bottle --merge --write --no-commit ./*.json
  cat "$tapdir/Formula/ophello.rb"
  git -C "$tapdir" add -A && git -C "$tapdir" -c user.name=spike -c user.email=spike@example.com commit -qm "ophello $v bottle"
  publish_repo "$tapdir"
  step "published $v"
  rclone lsl "$DST"
}

case "$1" in
  init)
    step "versions"; brew --version; git --version
    step "install rclone"; brew install rclone >/dev/null
    rclone lsf "$DST" 2>/dev/null && rclone purge "$DST" 2>/dev/null || true
    mkdir -p "$W"
    sha=$(make_src 1.0)
    git init -q --bare -b main "$BARE"
    git clone -q "$BARE" "$W/seed"
    write_formula 1.0 "$sha" "$W/seed"
    git -C "$W/seed" add -A && git -C "$W/seed" -c user.name=spike -c user.email=spike@example.com commit -qm "ophello 1.0"
    git -C "$W/seed" push -q origin HEAD:main
    step "tap local bare repo as $TAP"
    brew tap "$TAP" "$BARE"
    bottle_and_publish 1.0
    ;;
  release)
    v=$2
    sha=$(make_src "$v")
    tapdir=$(brew --repo "$TAP")
    write_formula "$v" "$sha" "$tapdir"
    brew uninstall "$TAP/ophello"
    bottle_and_publish "$v"
    ;;
  netprobe)
    step "network during build (opnet)"
    brew install --build-from-source "$TAP/opnet" && echo "RESULT: network AVAILABLE during install" || echo "RESULT: network BLOCKED during install"
    ;;
esac
