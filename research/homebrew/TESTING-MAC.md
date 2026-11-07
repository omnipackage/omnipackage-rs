# Testing the `macos` target on a Mac

The `macos` target is implemented, but has only been unit-tested on Linux. This doc is the first real run on macOS.

What is being tested: `omnipackage` builds a Homebrew bottle on the Mac and publishes it to the bucket, with the tap as a git repo served over dumb HTTP. Then an end user installs and upgrades it with plain `brew`.

## 1. Prerequisites

- An Apple Silicon Mac (Intel works too, but Homebrew no longer builds Intel bottles itself).
- Xcode Command Line Tools: `xcode-select --install`.
- Homebrew: <https://brew.sh>.
- Rust: `brew install rustup && rustup-init -y`, or any rustup install with a toolchain ≥ the `rust-version` in `Cargo.toml`.
- The working tree with the macOS changes. They are not committed yet, so copy the repo or commit/push from the Linux box first.
- The repo `.env` on the Mac, with at least `CLOUDFLARE_R2_ENDPOINT`, `CLOUDFLARE_R2_ACCESS_KEY_ID` and `CLOUDFLARE_R2_SECRET_ACCESS_KEY`. Add `CLOUDFLARE_ZONE_ID` and `CLOUDFLARE_API_TOKEN` to get CDN purges. The file is not in git, so copy it over a secure channel.

**The Mac must not already have `omnipackage-macos-hello` installed or tapped.** The build taps and installs the package under the same name end users get, `omnipackage/omnipackage-macos-hello`. If it's there, the build refuses with an untap hint.

## 2. Build omnipackage

```sh
cd omnipackage-rs
cargo build
```

You don't need to run `cargo test` on the Mac: Linux CI covers it. A few existing tests use Linux-only tools (for example `setsid`) and fail on macOS for unrelated reasons.

## 3. Automated end-to-end run (recommended)

```sh
sh tests/e2e/macos.sh
```

It uses the fixture `tests/fixtures/macos_project`, a C hello-world (`omnipackage-macos-hello`), and publishes to `repositories-test` under `macos-e2e/`. It stops at the first failure, printing `macos e2e: FAIL: …`. Success ends with `macos e2e: PASS`.

| Step | What it proves |
|---|---|
| `release` 1.0.0 | Builds the bottle with host brew, publishes the bottle, meta, tap and install page |
| `build` 1.0.1 into a second build dir | A build works with nothing installed; publishing later needs no brew |
| `brew tap` + `brew install` 1.0.0, then check for `Pouring` | Dumb-HTTP tap from R2 works and the bottle comes from `root_url` (no source build) |
| `publish` 1.0.1, then `brew update && brew upgrade` | Tap updates over dumb HTTP; the upgrade pours the new bottle |
| `brew test` | The formula's `test do` block passes |
| reinstall via `curl …/install.sh \| sh -s -- -y` | `install.sh` detects Darwin and runs the brew steps without sudo |

Knobs:
- `REPOSITORY="Local test"` publishes to `/tmp/omnipackage-macos-repo` instead of R2 (no network or credentials needed).
- `ENV_FILE=/path/.env` uses a different env file.
- `OMNIPACKAGE_BIN=…` uses a different binary.

The script uninstalls and untaps on exit. Its working dir, which holds the build dirs and logs, is printed at the end.

## 4. Manual run (for debugging a failing step)

Run from the repo root. `P` is the fixture and `O` is the binary.

```sh
P=tests/fixtures/macos_project; O=./target/debug/omnipackage; R="R2 test"
$O info --show-install-page-url --repository "$R" $P --env-file .env
$O release --distros macos --repository "$R" --build-dir /tmp/omp-mac $P --env-file .env
ls /tmp/omp-mac/omnipackage-macos-hello-macos/output/
cat /tmp/omp-mac/omnipackage-macos-hello-macos/homebrew.log
brew tap omnipackage/omnipackage-macos-hello https://repositories-test.omnipackage.org/macos-e2e/macos/tap.git
brew install omnipackage/omnipackage-macos-hello/omnipackage-macos-hello
omnipackage-macos-hello
```

To test an upgrade by hand:
1. `brew uninstall` and `brew untap` (the build needs the tap name free).
2. Bump `version.h` in a **copy** of the fixture.
3. Run `build`, then `brew tap` and `brew install` 1.0.0 again.
4. Run `publish` with the same `--build-dir`.
5. `brew update && brew upgrade`.

The e2e script does exactly this.

Expected bucket layout under `repositories-test/macos-e2e/`:

```
install.html  install.json  install.sh  badge.svg
macos/tap.git/{HEAD,config,info/refs,objects/…,refs/heads/main}
macos/bottles/omnipackage-macos-hello-<ver>.<tag>.bottle.tar.gz
macos/meta/<ver>/<tag>.json
```

There should be no `macos/src/`: sources are never published.

## 5. Things to record (open questions from the research)

1. **Bottle tag.** `ls …/output/*.bottle*.tar.gz` should show `arm64_<macos codename>` (e.g. `arm64_tahoe`). Note it down.
2. **Network during the build.** The question is whether brew's macOS sandbox blocks network inside `def install`. If it does, cargo and go projects that download dependencies at build time need vendoring. Probe it:
   ```sh
   cp -R tests/fixtures/macos_project /tmp/netprobe
   perl -pi -e 's|^    system "make"$|    system "curl", "-fsS", "-o", "/dev/null", "https://example.com"\n    system "make"|' /tmp/netprobe/.omnipackage/homebrew/formula.rb.liquid
   grep -n curl /tmp/netprobe/.omnipackage/homebrew/formula.rb.liquid
   ./target/debug/omnipackage build --distros macos /tmp/netprobe --env-file .env
   ```
   If the build succeeds, network is allowed. If it fails, check `homebrew.log` for the curl error.
3. **Developer mode is restored.** `brew bottle` silently turns developer mode on, and omnipackage turns it back off if it was off. Run `brew developer` before and after a build; the state should be the same.
4. **Older-bottle fallback (only if you have two macOS versions).** Does a bottle built on the older macOS get poured on the newer one?
5. **The install page.** Open the printed install page URL. A "macOS" group with the Apple logo should list the `brew tap …` / `brew install …` steps, and the badge should say `… 1 BREW`.

## 6. Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `omnipackage/omnipackage-macos-hello is already tapped from …` | A previous consumer install is still there: `brew uninstall omnipackage/omnipackage-macos-hello/omnipackage-macos-hello; brew untap omnipackage/omnipackage-macos-hello` |
| `Refusing to load formula … from untrusted tap` | You used the short name; install with the full `omnipackage/omnipackage-macos-hello/omnipackage-macos-hello`, or `brew trust omnipackage/omnipackage-macos-hello` |
| brew builds from source instead of pouring | No bottle for this OS/arch tag, which fails because sources aren't published (the formula `url` is the build host's `file://` path). Compare `brew info --json=v2 …` bottle tags against your system |
| `formula template must contain the line sha256 "{{ sha256 }}"` | The formula template's source `sha256` line was changed or removed |
| `brew update` doesn't see a new version | CDN caching of `macos/tap.git/info/refs`: set the `CLOUDFLARE_*` vars so publish purges, or wait and retry |
| `no macos build output at …, build it first` | `publish` was given a `--build-dir` with no prior `build` |

Build logs: `<build-dir>/omnipackage-macos-hello-macos/homebrew.log`.

## 7. Cleanup

```sh
brew uninstall --force omnipackage/omnipackage-macos-hello/omnipackage-macos-hello; brew untap omnipackage/omnipackage-macos-hello
ruby scripts/clear_bucket.rb repositories-test macos-e2e
rm -rf /tmp/omnipackage-macos-repo /tmp/omp-mac /tmp/netprobe
```

`brew trust` keeps an entry per tap URL. List the entries with `brew trust` and remove them with `brew untrust …`.

## 8. What to send back

- The `PASS`/`FAIL` line and, on failure, the failing step plus the tail of `homebrew.log`.
- The bottle tag from item 5.1 and the netprobe result from item 5.2.
- `brew --version` and `sw_vers -productVersion`.
