# packaging

**STATUS: PLANNED, no PKGBUILDs exist yet.** This directory holds the AUR
packaging plan; the actual recipes land at **M7** (ROADMAP), once release
artifacts exist to point at. The full design is `docs/CI-CD.md` §7.1.

## Layout (planned)

```
packaging/
├── aur/
│   ├── sweep-bin/          # prebuilt static musl release asset
│   │   ├── PKGBUILD
│   │   └── .SRCINFO
│   └── sweep/              # source build
│       ├── PKGBUILD
│       └── .SRCINFO
└── README.md               # this file
```

The AUR repository name must equal `pkgname`; each package is pushed to its own
`ssh://aur@aur.archlinux.org/<pkgname>.git` repo. This directory is the staging
copy, not the AUR repo.

## Package order

1. `sweep-bin`, static `x86_64-unknown-linux-musl` tarball from the GitHub
   release, verified with `sha256sums_x86_64`. No runtime dependencies.
2. `sweep`, source tarball (`sweep-v<ver>-src.tar.gz`) built with
   `cargo build --release --locked` (**not** `--frozen`), then
   `cargo test --release --locked` in `check()`.

## Rules

- `arch=('x86_64')` only; aarch64 binaries are served from GitHub Releases.
- Both packages install `/usr/bin/sweep`, so they must `conflicts` on each
  other; `sweep-bin` also `provides=('sweep')`.
- `.SRCINFO` is generated (`makepkg --printsrcinfo`), never hand-edited.
- Use release tarballs, not `archive/refs/tags/...`.
- `packaging.yml` validates the recipes in `archlinux:base-devel` as a non-root
  user (`makepkg`, `namcap`, `.SRCINFO` diff). It skips cleanly until the files
  exist.
- Installing the man page, completions, and systemd units is part of `package()`
  at M7. The timer ships **disabled**.

## Publishing

First upload is manual. The sequence is in `docs/CI-CD.md` §6.5. Do not add an
AUR push workflow before the packages exist and the release workflow has been
dry-run successfully.
