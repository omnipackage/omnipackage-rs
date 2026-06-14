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
- **`version_extractors`** regex runs against the **whole file**; use one capture group and a
  unique prefix (e.g. `project(` to avoid matching `cmake_minimum_required(VERSION 3.21)`).
- **`runtime_dependencies` is usually empty** — `rpmbuild`/`dpkg-shlibdeps` auto-detect linked
  libraries. List only `dlopen`ed libs (QML modules!), external tools, fonts/themes.
- **`before_build_script`** runs in-container before the build — enable extra repos (EPEL/CRB)
  or install a newer toolchain.
- **The build needs network + git** at configure time if the project fetches deps
  (CPM/FetchContent/Go modules/cargo). omnipackage containers have network.
- **Valid distro IDs:** run `omnipackage info --list-distros`, or see
  <https://docs.omnipackage.org/distros/>.

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
