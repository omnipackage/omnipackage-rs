[![CI](https://github.com/omnipackage/omnipackage-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/omnipackage/omnipackage-rs/actions/workflows/ci.yml)

## OmniPackage CLI

Build and distribute RPM, DEB & Arch packages [easily](https://omnipackage.org/about).

## Installation

| Channel | x86_64 | aarch64 |
|---|---|---|
| **Stable** (recommended for most users) | [![stable repository](https://repositories.omnipackage.org/omnipackage-rs/stable/badge.svg)](https://repositories.omnipackage.org/omnipackage-rs/stable/install.html) | [![stable aarch64 repository](https://repositories.omnipackage.org/omnipackage-rs/stable-aarch64/badge.svg)](https://repositories.omnipackage.org/omnipackage-rs/stable-aarch64/install.html) |
| **Master** (unstable builds from master) | [![master repository](https://repositories.omnipackage.org/omnipackage-rs/master/badge.svg)](https://repositories.omnipackage.org/omnipackage-rs/master/install.html) | [![master aarch64 repository](https://repositories.omnipackage.org/omnipackage-rs/master-aarch64/badge.svg)](https://repositories.omnipackage.org/omnipackage-rs/master-aarch64/install.html) |

- [AUR](https://aur.archlinux.org/packages/omnipackage) (Arch Linux users who prefer AUR)
- From sources `cargo build --release`

## GitHub Action

Install the omnipackage CLI in a workflow (Ubuntu runners; CPU architecture auto-detected). The channel is the ref you pin: `@stable` for stable, `@master` for master builds.

```yaml
- uses: omnipackage/omnipackage-rs@stable
- run: omnipackage release .
```

```yaml
- uses: omnipackage/omnipackage-rs@master   # master (unstable) builds
- run: omnipackage release .
```

## Documentation

Visit https://docs.omnipackage.org

## Use with AI agents

**Claude Code** — install the omnipackage skill as a plugin (auto-triggers on packaging tasks, auto-updates):

```
/plugin marketplace add omnipackage/omnipackage-rs
/plugin install omnipackage@omnipackage
```

**Any other agent** — point it at the full docs in one file: https://docs.omnipackage.org/llms-full.txt
