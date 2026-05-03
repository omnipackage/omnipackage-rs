# omnipackage e2e test

End-to-end happy-path test for `omnipackage init` + build + install.
For each project type, generates a hello-world package, installs it in a
fresh distro container, runs the binary, and asserts the output line.

This is a **manual developer tool** — not run by `cargo test`.

## Run

```sh
bash tests/e2e/run.sh
```

Defaults: 8 project types × 7 distros (one recent per family) = 56 builds.
Plan on ~2 hours for a full clean run.

## Knobs

| Env var      | Default                              | Notes                                          |
|--------------|--------------------------------------|------------------------------------------------|
| `TYPES`      | `c cpp cmake rust go python ruby crystal` | Restrict to a subset for faster runs       |
| `DISTROS`    | `opensuse_tumbleweed fedora_42 debian_13 ubuntu_24.04 almalinux_10 rockylinux_9 mageia_9` | Restrict distros |
| `RUNTIME`    | `podman`                             | Set to `docker` if you don't have podman       |
| `VERBOSE`    | `0`                                  | `1` streams build output instead of capturing  |
| `TMP_ROOT`   | `/tmp/omnipackage-e2e`               | Where workdir/build_dir/repo/logs go — point elsewhere if `/tmp` is small (a full run can use 5–20 GB). All artifacts stay there for inspection; clean up with `rm -rf $TMP_ROOT`. |
| `OMNIPACKAGE_BIN` | `target/debug/omnipackage`      | Override binary path                           |

```sh
# Quick smoke
TYPES=rust DISTROS=fedora_42 bash tests/e2e/run.sh

# Wider coverage on rpm only
DISTROS="fedora_42 almalinux_10 rockylinux_9 opensuse_tumbleweed" \
    bash tests/e2e/run.sh

# Debug a specific failure
TYPES=python DISTROS=debian_13 VERBOSE=1 bash tests/e2e/run.sh
```

## What it does per (type, distro)

1. Copy `tests/e2e/projects/<type>/` → tmpdir.
2. `omnipackage init <tmpdir>` → scaffolds `.omnipackage/`. **No `--type`** —
   detection is part of what we're testing. The fixture's marker files
   (`Cargo.toml`, `go.mod`, `CMakeLists.txt`, etc.) must lead to the right
   project type; mismatch is reported as `detect-<wrong-type>` failure.
3. Redirect the generated `config.yml` localfs path from `${HOME}/<pkg>-repo`
   to `$TMP_ROOT/omni-e2e-<type>-repo`.
4. `omnipackage gpg generate ... --format base64` → write `GPG_KEY=...` to
   `<tmpdir>/.env` so the publish stage has a key to sign with.
5. `omnipackage release <tmpdir> --repository "Local test" --distros <distros>`
   — exercises build + publish + signing end-to-end.
6. Locate artifact in `<repo_dir>/<distro>/**/*.{rpm,deb}`.
7. Spin up fresh distro container, mount artifact, install via the distro's
   native package manager (so runtime deps get pulled), run
   `helloworld-<type>`, grep for `hello from <type> 1.2.3`.

## Distros chosen (one per family)

| Family     | Distro id              | Image                |
|------------|------------------------|----------------------|
| openSUSE   | `opensuse_tumbleweed`  | `opensuse/tumbleweed`|
| Fedora     | `fedora_42`            | `fedora:42`          |
| Debian     | `debian_13`            | `debian:trixie`      |
| Ubuntu     | `ubuntu_24.04`         | `ubuntu:24.04`       |
| AlmaLinux  | `almalinux_10`         | `almalinux:10`       |
| Rocky      | `rockylinux_9`         | `rockylinux:9`       |
| Mageia     | `mageia_9`             | `mageia:9`           |

## What it does NOT cover

- electron, tauri (GUI apps; headless verification is a separate problem).
- generic (no detection target by definition).
- Multi-version distros within a family.
- Publish flow (S3, repository signing) — `release` not exercised.
- Upgrade scenarios.

## Troubleshooting

Everything from a run lives under `$TMP_ROOT/<type>/`:

```
<type>/
  source/    fixture + .omnipackage/ (rendered config) + .env
  build/     per-distro container output (rpmbuild / debuild trees)
  repo/      localfs repo with the published .rpm/.deb under <distro>/
  logs/      init.log, gpg.log, release.log, <distro>.verify.log
```

Re-running a type wipes `$TMP_ROOT/<type>/` fresh. Clean up everything
with `rm -rf $TMP_ROOT` when you're done.

Common failure modes:
- **build fails for distro X**: usually a missing build dep — inspect
  `<log>.build.log` and patch the embedded template at
  `src/init/templates/<type>/config.yml`.
- **install fails in container**: runtime dep missing. Check the
  `verify.log` for `Unable to locate package` (apt) / `nothing provides`
  (dnf). Add to `runtime_dependencies:` in the template.
- **binary not found**: the spec/rules `%files` or `install` paths don't
  match the slug. Rare, but indicates a template bug.
