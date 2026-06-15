---
name: omnipackage
description: >
  Use when packaging a project as native Linux RPM/DEB/pacman with omnipackage —
  scaffolding or filling the .omnipackage/ config, building/publishing packages
  across distros (Fedora, openSUSE, Debian, Ubuntu, Arch, Manjaro, …), or debugging
  omnipackage build failures. Triggers: "omnipackage", ".omnipackage", "package for
  Linux", "build rpm/deb", "PKGBUILD/Arch/pacman", "omnipackage init/build/release".
---

# omnipackage

`omnipackage` builds native RPM, DEB, and pacman packages for many Linux distros by compiling
the project inside a per-distro container (Fedora, openSUSE, RHEL-clones, Mageia, Debian,
Ubuntu, Arch, Manjaro). Config lives in `.omnipackage/`:

```
.omnipackage/
  config.yml                # metadata, version, per-distro build deps, repos, builds list
  specfile.spec.liquid      # RPM spec (Liquid template)
  deb/
    control.liquid          # DEB control (package metadata)
    rules.liquid            # debian/rules (build steps) — TABS, not spaces
    changelog.liquid
    compat.liquid
  PKGBUILD.liquid           # pacman/Arch build (Liquid); normal build()/package() PKGBUILD
```

Templates are [Liquid](https://shopify.github.io/liquid/) rendered at build time:
`{{ package_name }}`, `{{ version }}`, `{{ build_dependencies | join: ' ' }}`, etc.

## When to use

- Setting up packaging for a project (`omnipackage init`, then filling the gaps).
- Adding/fixing `build_dependencies` or `runtime_dependencies` per distro.
- A build fails (missing package, "unpackaged files", FetchContent/link error, OOM).
- Building, signing, or publishing packages.

## Workflow

1. **`omnipackage init`** — scaffolds `.omnipackage/` and detects the project type
   (CMakeLists.txt → cmake, Cargo.toml → rust, …). The version_extractor regex is pre-filled.
2. **Fill the gaps** in `config.yml`: real `homepage` / `description`; per-distro-family
   `build_dependencies` (the main work — names differ per family); trim `builds:` to the
   distros you actually ship.
3. **Build ONE rpm + ONE deb first** to shake out problems early:
   `omnipackage build --distros fedora_42` and `omnipackage build --distros debian_13`. A local
   `cmake`/`make` does **not** catch container-only issues — only a real container build does.
4. **Verify the package contents** (see Build recipes → Verifying a built package) before the full matrix.
5. **Iterate**, then build the remaining distros.
6. **Publish** (optional): generate a signing key once
   (`omnipackage gpg generate -n "Name" -e you@example.com --format base64` → put in `.env`),
   configure a repository, then `publish` or `release` (build + publish in one pass).

## config.yml pattern

DRY the `builds:` list with three layers of YAML anchors: `common` → per-format → per-family.
Package names diverge per **distro family**, which is the whole reason for family anchors.

```yaml
common: &common
  package_name: "myapp"
  maintainer: "You <you@example.com>"
  homepage: "https://github.com/you/myapp"
  description: "Short description"

fedora_rpm: &fedora_rpm
  <<: *common
  build_dependencies: [gcc-c++, cmake, make, git, qt6-qtbase-devel]
  rpm: { spec_template: ".omnipackage/specfile.spec.liquid" }

deb: &deb
  <<: *common
  build_dependencies: [build-essential, cmake, git, qt6-base-dev]
  deb: { debian_templates: ".omnipackage/deb" }

pacman: &pacman                     # arch + manjaro share one anchor (same package names)
  <<: *common
  build_dependencies: [cmake, make, git, qt6-base]   # Arch names; base-devel is preinstalled
  pacman: { pkgbuild_template: ".omnipackage/PKGBUILD.liquid" }

builds:
  - { distro: "fedora_42", <<: *fedora_rpm }
  - { distro: "debian_13", <<: *deb }
  - { distro: "arch",      <<: *pacman }
  - { distro: "manjaro",   <<: *pacman }
```

Key points:
- **`version_extractors`** is a required top-level field with three providers — `file` (regex
  over a file; runs against the **whole file**, use one capture group + a unique prefix like
  `project(` to dodge `cmake_minimum_required(VERSION 3.21)`), `shell` (trimmed stdout), and
  `constant` (hardcoded). See Field notes for the git-describe/CI pattern.
- **`runtime_dependencies` is usually empty** — `rpmbuild`/`dpkg-shlibdeps` auto-detect linked
  libraries. List only `dlopen`ed libs (QML modules! PCSC CCID driver! pcscd daemon), external
  tools, fonts/themes.
- **`before_build_script`** runs in-container before the build — install a newer toolchain or
  enable extra repos (EPEL/CRB). **But it runs AFTER `build_dependencies` are installed**, so
  packages from a repo disabled by default (RHEL CRB/PowerTools) can't be plain
  `build_dependencies` — enable the repo and `dnf install` them inside the script. See Field notes.
- **The build needs network + git** at configure time if the project fetches deps
  (CPM/FetchContent/Go modules/cargo). omnipackage containers have network.
- **Valid distro IDs:** run `omnipackage info --list-distros`, or see
  <https://docs.omnipackage.org/distros/>.

## Field notes

Hard-won specifics the docs don't spell out. (Example domain: a Go + Fyne cgo GUI app linking
PCSC/smartcard + OpenGL/X11 — i.e. the trickiest case, mixing cgo, a toolchain newer than distros
ship, and libs whose names diverge wildly.)

**Go / cgo desktop apps.** `omnipackage init` detects `go.mod` and scaffolds Go templates +
`install_go.sh` (distro go is often too old for modern `go.mod`).
- The scaffolded version_extractor reads `version.go` (`Version = "x.y.z"`); projects without that
  file fail at version resolution — switch the provider.
- Build with `export GOTOOLCHAIN=auto` (fetches the exact `go 1.xx` from `go.mod` at build time;
  needs network — containers have it). Don't rely on the distro go matching `go.mod`.
- `install_go.sh`'s curl download breaks on images with a broken libcurl (seen on **Tumbleweed**:
  `undefined symbol: ngtcp2_...`). Fix: install distro `go`/`golang` instead, and let
  `GOTOOLCHAIN=auto` upgrade it. Make the script accept distro go when `>= 1.21`, else download.
- **`GOSUMDB=off`** on some distros (**Mageia**) blocks the GOTOOLCHAIN toolchain download
  (`checksum database disabled by GOSUMDB=off`). Force it back in the build env:
  `export GOSUMDB=sum.golang.org` and `export GOPROXY=https://proxy.golang.org,direct`.
- In `debian/rules`: set `GOPATH`/`GOCACHE` inside the build tree (HOME may be read-only), and add
  `override_dh_dwz:` + `override_dh_auto_test:` for stripped (`-ldflags "-s -w"`) Go binaries.
- A plain `go build` binary + installing the `.desktop`/icon yourself is enough — no `fyne package`.

**Per-family package names** (the names that actually bit, for PCSC + OpenGL/X11):

| need | Debian/Ubuntu | Fedora | RHEL clones | openSUSE | Mageia | Arch |
|------|---------------|--------|-------------|----------|--------|------|
| go | `golang-go` | `golang` | `golang` | `go` | `golang` | `go` |
| pkg-config | `pkg-config` | `pkgconf-pkg-config` | `pkgconf-pkg-config` | `pkg-config` | `pkgconf` | `pkgconf` |
| Mesa GL dev | `libgl1-mesa-dev` | `mesa-libGL-devel` | `mesa-libGL-devel` | `Mesa-libGL-devel` | `lib64mesagl-devel` | `mesa` |
| X11 dev (Fyne/glfw) | `xorg-dev` | `libX{11,cursor,randr,inerama,i,xf86vm}-devel` | same (CRB) | same (capital X) | `lib64x{11,cursor,…}-devel` | `libx11 libxcursor …` |
| pcsc dev | `libpcsclite-dev` | `pcsc-lite-devel` | `pcsc-lite-devel` (CRB) | `pcsc-lite-devel` | `lib64pcsclite-devel` | `pcsclite` |
| pcscd daemon (runtime) | `pcscd` | `pcsc-lite` | `pcsc-lite` | `pcsc-lite` | `pcsc-lite` | `pcsclite` |
| CCID driver (runtime, dlopen'd) | `libccid` | `pcsc-lite-ccid` | `pcsc-lite-ccid` | **`pcsc-ccid`** | `ccid` | `ccid` |

Traps: openSUSE's CCID driver is `pcsc-ccid`, **not** `pcsc-lite-ccid` (Fedora's name) — both build
fine but the openSUSE package won't install. The pcscd daemon + CCID driver are dlopen'd at runtime,
so they're **never** auto-detected — always explicit `runtime_dependencies`. Verify any uncertain
name in seconds: `docker run --rm <image> <dnf|zypper|pacman> ... search/list <pkg>`.

**RHEL clones (Alma/Rocky).** X11 + pcsc-lite `-devel` live in CRB (disabled by default), and
build_dependencies install *before* `before_build_script` — so put only base packages
(`gcc gcc-c++ git curl`) in `build_dependencies` and install the `-devel` libs (+`golang`) in the
script after `dnf config-manager --set-enabled crb || ... powertools` and `dnf install epel-release`.
rpmbuild only checks listed `BuildRequires`, so libs the script installs (absent from
build_dependencies) still satisfy cgo.

**deb compat floor.** Targeting old Ubuntu (20.04 = debhelper 12) means `Build-Depends: debhelper
(>= 13)` / `compat 13` fails with *"Unmet build dependencies"*. Use **12** (works Ubuntu 20.04 →
Debian 13); explicit `override_dh_*` rules don't depend on the compat level.

**Version from git tag + CI.** Mirror a release pipeline with a `shell` extractor:
`command: "git describe --tags --abbrev=0 2>/dev/null | tail -c +2 | grep . || echo 0.0.0"` (strips
`v`, falls back so tagless local checkouts still build). The extractor runs **host-side before
staging**, so `.git` is available even though it's excluded from the container. In GitHub Actions
the default shallow checkout has no tags → `git describe` returns nothing; set `actions/checkout`
`fetch-depth: 0` + `fetch-tags: true`, **and** ensure the tags exist on the checked-out remote (a
fork created without tags has none — `git push origin --tags`).

**Misc.** Verify a `.deb` on a non-Debian host (no `dpkg`): `ar x pkg.deb` then
`tar --zstd -tf data.tar.*` (extract `control` from `control.tar.*` for Depends). Build many distros
in **waves of ~3** — parallel cgo/Qt compiles can OOM.

## Reference

Full, current documentation (this skill is intentionally thin — read the docs for depth):

- **Everything in one file (LLM-friendly):** <https://docs.omnipackage.org/llms-full.txt>
- **Build recipes** (CMake/Qt, Electron, pacman, Qt6 dependency map, patching source, verifying
  packages): <https://docs.omnipackage.org/guides/build_recipes/>
- **Troubleshooting** (symptom → fix, finding the right package name):
  <https://docs.omnipackage.org/guides/troubleshooting/>
- **Templates** (Liquid variables, per-distro custom fields): <https://docs.omnipackage.org/guides/templates/>
- **Configuration** (`config.yml` schema): <https://docs.omnipackage.org/configuration/>
- **CLI reference**: <https://docs.omnipackage.org/cli/>
- **CI/CD** (GitHub Actions matrix): <https://docs.omnipackage.org/guides/cicd/>
- Reference configs: [mpz](https://github.com/olegantonyan/mpz/tree/master/.omnipackage),
  [rssguard](https://github.com/olegantonyan/rssguard/tree/master/.omnipackage),
  [pulsar (Electron)](https://github.com/olegantonyan/pulsar/tree/master/.omnipackage).
