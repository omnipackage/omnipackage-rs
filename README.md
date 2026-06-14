[![CI](https://github.com/omnipackage/omnipackage-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/omnipackage/omnipackage-rs/actions/workflows/ci.yml) [![OmniPackage repositories badge](https://repositories.omnipackage.org/omnipackage-rs/stable/badge.svg)](https://repositories.omnipackage.org/omnipackage-rs/stable/install.html) [![OmniPackage repositories badge](https://repositories.omnipackage.org/omnipackage-rs/master/badge.svg)](https://repositories.omnipackage.org/omnipackage-rs/master/install.html)

## OmniPackage CLI

Build and distribute RPM, DEB & Arch packages [easily](https://omnipackage.org/about).

## Installation

- [Stable binaries](https://repositories.omnipackage.org/omnipackage-rs/stable/install.html) (recommended for most users)
- [Unstable builds from master](https://repositories.omnipackage.org/omnipackage-rs/master/install.html)
- [AUR](https://aur.archlinux.org/packages/omnipackage) (Arch Linux users who prefer AUR)
- From sources `cargo build --release`

## Documentation

Visit https://docs.omnipackage.org

## Use with AI agents

**Claude Code** — install the omnipackage skill as a plugin (auto-triggers on packaging tasks, auto-updates):

```
/plugin marketplace add omnipackage/omnipackage-rs
/plugin install omnipackage@omnipackage
```

**Any other agent** — point it at the full docs in one file: https://docs.omnipackage.org/llms-full.txt
