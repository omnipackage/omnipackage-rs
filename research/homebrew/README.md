# macOS Homebrew support — research handoff

Status: implemented as the `macos` target (`src/target/mac/`, 2026-11-07), not yet run on a real Mac. The macOS e2e test is `tests/e2e/macos.sh`, using the fixture `tests/fixtures/macos_project`.

## Goal

Give macOS users the same experience as the Linux repos: add a repository once, `install`, then get updates through the native package manager. Builds and repositories stay on the project's own S3 bucket.

## Verdict

It works, but it is a **separate architecture**:

- The only things shared with the Linux path are the `config.yml` entry point and the bucket.
- macOS can't run in a container on Linux, so builds run natively on a macOS host (a CI runner such as GitHub `macos-15`).
- Signing works differently: no GPG, see below.
- The repository layout is different: one formula covers all macOS versions, where Linux has one directory per distro.

## Homebrew facts that shape the design

- **Tap.** A tap must be something git can clone, but not necessarily on GitHub: `brew tap user/repo <any-git-url>`. A bare repo served as static files over git's "dumb HTTP" protocol (after `git update-server-info`) works, including from S3 behind Cloudflare. Verified in the spike.
- **Payloads.** A formula's `url`, a bottle's `root_url` and a cask's `url` are plain HTTPS downloads, so S3 works for all of them.
- **Tap trust (Homebrew 6.0, June 2026).**
  - Tapping no longer lets a third-party tap's code run.
  - Installing by short name is refused: `Refusing to load formula … from untrusted tap`.
  - `brew install user/tap/formula` trusts just that formula, and the trust is recorded by tap URL. It survived the upgrade in the spike.
  - The alternative is `brew trust user/tap`.
  - The env-var escape hatches are deprecated.
- **No signature mechanism for taps or formulae.**
  - Integrity comes from the formula's `sha256`; authenticity from HTTPS plus tap trust.
  - Homebrew 7.0 verifies bottle attestations only "for supported third-party taps"; requirements undocumented, probably GitHub-only.
  - Bottles from S3 with no attestation installed without errors.
- **Bottles.**
  - Built with `brew install --build-bottle`, then `brew bottle --json --root-url=…`.
  - `brew bottle --merge --write --no-commit <json>` writes the `bottle do` block into the formula.
  - The uploaded name differs from the local file: `ophello-1.1.x86_64_linux.bottle.tar.gz` versus local `ophello--1.1…`. Use `filename` from the JSON.
- **Platforms.**
  - Apple Silicon (arm64) is the only target worth building:
    - macOS Intel moved to Tier 3 in September 2026: no CI, no new bottles.
    - Homebrew stops running on Intel entirely in September 2027.
  - Tier 1 is Sequoia 15, Tahoe 26 and Golden Gate 27.
- **Only the latest version is kept.** Homebrew keeps one version per formula; a pinned major needs a versioned formula like `myapp@1`. omnipackage's `retain_packages` mostly doesn't apply.
- **Casks (GUI `.app`).**
  - Downloads are quarantined, and `--no-quarantine` is deprecated since 5.0.
  - Homebrew disables official casks that fail Gatekeeper since September 2026.
  - In practice a cask needs Developer ID signing and notarization. `rcodesign` can do both from Linux.
- **Binary formulae are an anti-pattern** (formulae that just copy a prebuilt tarball). GoReleaser deprecated `brews` in favour of `homebrew_casks`. They also break for anything linking against Homebrew libraries.
- **Installing a formula from a local path** requires `HOMEBREW_DEVELOPER` (since 5.0). A tap is the only realistic way to distribute.

## Chosen design

### End-user experience

```sh
brew tap acme/myapp https://repo.acme.com/myapp/homebrew-tap.git
brew install acme/myapp/myapp
brew upgrade          # later, as with apt upgrade
```

The generated `install.html`, `install.json` and `install.sh` should show this.

`install.sh` today detects the distro via `/etc/os-release` and installs through sudo. It needs a `Darwin` branch, and that branch must not use sudo, because Homebrew refuses to run as root.

### Bucket layout (proposal)

```
<path_in_bucket>/
  homebrew-tap.git/        # bare repo + info/refs (dumb HTTP); Formula/<name>.rb
  homebrew/bottles/        # <name>-<ver>.<tag>.bottle[.N].tar.gz  (immutable names)
```

### Publish pipeline (runs on macOS host)

1. Don't upload the source tarball: sources must not be published to the repository. The formula's `url` stays the build host's `file://` tarball (the bytes the bottle was built from), so users without a matching bottle cannot build from source.
2. Fetch the published tap repo, or create it on first publish.
3. Render the formula from a Liquid template: `url`, `sha256`, `depends_on`, `install` block.
4. Tap the **local** working copy under the final tap name, so nothing is published before the bottle exists.
5. Build the bottle:
   ```sh
   brew install --build-bottle <tap>/<name>
   brew bottle --json --root-url=<bucket>/homebrew/bottles <tap>/<name>
   ```
6. Upload the bottle, using the JSON's `filename`.
7. Write the bottle block into the formula with `brew bottle --merge --write --no-commit`, then commit.
8. Push to the bare repo and run `git update-server-info`.
9. Sync to S3: **bottle first, then objects, then refs.** Clients must never see a ref pointing at something missing.
10. Purge the Cloudflare cache where needed. Extensionless git files weren't cached in the spike, but purge `info/refs` and `HEAD` to be safe.

Wrap steps 2–9 in `src/publish/locking.rs`: parallel jobs (arm64, maybe Intel) merge into the same formula.

### Config sketch (proposal, not final)

```yaml
homebrew: &homebrew
  <<: *common
  build_dependencies: [cmake]        # brew names -> depends_on ... => :build
  runtime_dependencies: [qt]         # -> depends_on
  homebrew: { formula_template: ".omnipackage/formula.rb.liquid" }

builds:
  - { distro: "macos_arm64", <<: *homebrew }
```

The build target needs a new executor beside `src/runner.rs`. It runs on the host: no container, no image. It should refuse to run when the host isn't Darwin.

### Rejected alternatives

- **Tap on GitHub** (`owner/homebrew-tap`): gives a shorter `brew tap` and probably attestations, but needs a push token and a second provider. Fallback only if a macOS test breaks the S3 tap.
- **Binary formula:** Homebrew considers it an anti-pattern, and it breaks on linked Homebrew libraries.
- **Cross-compiling from Linux** (osxcross/zig): `brew bottle` can't run there, GUI frameworks fail, and redistributing the SDK is a licensing grey area.
- **Tart macOS VMs** for local isolation: possible later phase. Apple Silicon only, at most 2 VMs per Mac.
- **Casks:** a phase 2 for GUI apps, gated on signing and notarization.

## Spike

`spike/` reproduces the whole flow with Homebrew on Linux in podman. It writes to `repositories-test/homebrew-spike/` (public at `https://repositories-test.omnipackage.org/homebrew-spike/`).

```sh
bash research/homebrew/spike/run.sh
```

- `run.sh`:
  - reads only `CLOUDFLARE_R2_*` from the repo `.env`;
  - starts a publisher container (with credentials) and a consumer container (without);
  - runs the phases in order.
- `scripts/publisher.sh`:
  - `init`: source 1.0, tap repo, bottle, publish.
  - `release <ver>`: new version, new bottle, republish.
  - `netprobe`: does a formula `install` block have network?
  - Uploads with `rclone`, installed via brew.
- `scripts/consumer.sh`:
  - `install`: tap via dumb HTTP, short name (refused), full name (bottle poured).
  - `upgrade`: `brew update` then `brew upgrade`.
- `run1.log`: full output of the successful run. It was checked to contain no secret values.

Results:

| Check | Result |
|---|---|
| `brew tap` from dumb-HTTP repo on R2/Cloudflare | ✅ |
| Install pours bottle from S3 `root_url`, no attestation error | ✅ |
| `brew update` sees new commit immediately (no CDN staleness) | ✅ |
| `brew upgrade` 1.0 → 1.1 pours the new bottle | ✅ |
| Short-name install on untrusted tap | ❌ refused (expected) |
| Network inside `install` during source build | available on Linux in container (not representative of macOS) |

Quirk: the first bottle came out as `rebuild 1` (`…bottle.1.tar.gz`). omnipackage should control the rebuild number explicitly through `brew bottle` flags.

Cleanup: `ruby scripts/clear_bucket.rb repositories-test homebrew-spike`, and `podman rm -f spike-pub spike-con` if a run was interrupted.

## Open questions (need a Mac)

1. Bottle tag on Apple Silicon (`arm64_sequoia`, etc.), and whether a bottle built on Sequoia is chosen on Tahoe and Golden Gate.
2. Whether the macOS build sandbox blocks network during `install`. If it does, cargo, go and npm projects need vendored dependencies or `resource` blocks, unlike the Linux containers. Run `publisher.sh netprobe` natively.
3. Isolation on a dev Mac: the publisher uses the host's real Homebrew. That's fine on CI; locally, clean up with `brew untap omnitest/spike`.

`run.sh` is podman-only. On a Mac, run `publisher.sh` and `consumer.sh` directly after replacing the `/scripts/common.sh` path.

## Sources

- https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap
- https://docs.brew.sh/Bottles
- https://docs.brew.sh/Tap-Trust
- https://brew.sh/2025/11/12/homebrew-5.0.0
- https://brew.sh/2026/06/11/homebrew-6.0.0/
- https://brew.sh/2026/09/13/homebrew-7.0.0/
- https://www.helpnetsecurity.com/2026/09/15/homebrew-7-0-0/
- https://github.com/Homebrew/brew/pull/23769
- https://goreleaser.com/deprecations/
