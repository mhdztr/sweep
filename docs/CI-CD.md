# sweep: CI/CD, security, and distribution

> **STATUS: foundation IMPLEMENTED (M1.5, 2026-10-20); release/AUR parts
> PLANNED.** All five workflows exist: `ci.yml` (fmt / clippy / test /
> doctest / docs / release-build / msrv), `security.yml` (deny, machete,
> actionlint, typos, dependency-review), `scheduled.yml` (CodeQL, Scorecard,
> beta), `release.yml` (tag-gated scaffold with dry run), and
> `packaging.yml` (skeleton until M7). `deny.toml`, `SECURITY.md`,
> `CHANGELOG.md`, and `.github/dependabot.yml` are in place. Still planned
> for **M7**: PKGBUILDs + `.SRCINFO`, the optional `aur` job, and the first
> real tag. GitHub-side settings (branch/tag protection, secret scanning,
> `release` environment reviewer) are maintainer steps; see §8 and
> `ROADMAP.md`. Unchecked tasks live in `ROADMAP.md`; this file holds the
> detail so a future session does not have to re-derive it.
>
> **Scope note:** the `flatpak-unused` *cleanup rule* (`docs/RULES-CATALOG.md`
> §16) is unrelated to *Flatpak packaging*. The rule stays. Flatpak packaging is
> deferred by decision D11 (`CONTEXT.md`); see §7.3.

---

## 1. Goals and non-goals

**Goals**

1. Every push is verified on a machine that is not the developer's (fmt, clippy,
   tests, doctests, docs, release build).
2. Supply chain is visible and gated: known-vulnerable dependencies, license
   drift, unused crates, leaked secrets, risky workflow changes.
3. Releases are built from a tag, checksummed, and attributable (SBOM + SLSA
   provenance attached to the artifacts).
4. Arch users install through the AUR; everyone else gets a static binary.
5. Idle cost is zero: nothing publishes unless a `v*` tag is pushed.

**Non-goals (for 0.1)**

- Flatpak packaging (D11: sandbox conflicts, §7.3).
- crates.io publishing (§7.4).
- Windows/macOS CI (the crate is Linux-only by design).
- A coverage *gate* (report first; a percentage gate rewards line-touching over
  safety invariants).
- Miri, fuzzing, `cargo-vet`/`crev`, `cargo-semver-checks` (see §1.1).

### 1.1 Explicitly rejected for now

| Idea | Why not |
|---|---|
| Miri | Tests perform real filesystem syscalls; Miri cannot run them. |
| `cargo-fuzz` | Only parser-ish surface is path rendering + config TOML. Revisit with `proptest` unit tests for `truncate`/`display_width` if desired. |
| Coverage gate | Measure first; gate only if it proves meaningful on `src/scan` + `src/rules`. |
| `cargo-vet` / `crev` | Trust-maintenance overhead not justified for a single maintainer. |
| `cargo-semver-checks` | Ships a binary, not a library API. |
| Windows/macOS runners | Linux-only code (`/proc`, statvfs, libc). Red builds with no signal. |
| crates.io | A root-capable cleaner is better installed from a distro package. Add later only on demand. |

---

## 2. Baseline (2026-10-19)

What `.github/workflows/ci.yml` does today:

- Trigger: `push` to `main` + all `pull_request`.
- Single `check` job on `ubuntu-latest`, stable Rust + rustfmt + clippy,
  `Swatinem/rust-cache`, then:
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --all-targets`, and a redundant `cargo test --test cli_smoke`
  mislabelled "Docs sync".

Known gaps this plan closes:

| Gap | Consequence |
|---|---|
| No `--locked` | A drifted `Cargo.lock` can pass CI and fail for packagers. |
| `--all-targets` excludes doctests | Broken doc examples ship unnoticed. |
| No `cargo doc` | Broken intra-doc links accumulate. |
| No release build in CI | `lto`/`strip`/release-only breakage appears at tag time. |
| No security scanning | Vulnerable or mislicensed crates merge silently. |
| No artifacts / SBOM / provenance | AUR `-bin` cannot be packaged from a release. |
| No tag trigger | Releases are hand-built and unverifiable. |
| No action hygiene | Workflows are linted by nothing; actions float on tags. |
| No scheduled run | New advisories are invisible until the next commit. |

---

## 3. Workflow map

Five workflows, split by concern so the PR gate stays fast.

| File | Trigger | Purpose | Target time |
|---|---|---|---|
| `.github/workflows/ci.yml` | push `main`, PR | correctness gate | < 5 min |
| `.github/workflows/security.yml` | PR (subset), weekly cron, dispatch | vulns, licenses, workflow hygiene | < 6 min |
| `.github/workflows/release.yml` | tag `v*`, manual dispatch | build, package, attest, publish | ~15 min |
| `.github/workflows/packaging.yml` | PR touching `packaging/**`, dispatch | AUR PKGBUILD validation (skipped until M7) | ~4 min |
| `.github/workflows/scheduled.yml` | weekly cron, dispatch | CodeQL, Scorecard, beta toolchain | ~15 min |

Shared conventions for every workflow:

- `permissions: contents: read` at the top; escalate per job only.
- `concurrency` per ref with `cancel-in-progress: true` (release: `false`).
- `timeout-minutes` on every job.
- `CARGO_TERM_COLOR: always`; `--locked` on every command that resolves
  dependencies (build, test, clippy, doc). `cargo fmt` does not take it.
- Third-party actions pinned to commit SHAs (Dependabot keeps them fresh);
  GitHub-owned actions may stay on major tags.
- No long-lived secrets in build/test jobs. Only the publish job gets
  `contents: write`; nothing else escalates unless the job needs it.

---

## 4. `ci.yml`: correctness gate

| Job | Command(s) | Notes |
|---|---|---|
| `fmt` | `cargo fmt --all -- --check` | fastest failure first |
| `clippy` | `cargo clippy --all-targets --all-features -- -D warnings` | `--all-features` is a no-op today, future-proofs |
| `test` | `cargo test --all-targets --locked` | lib + integration suites |
| `doctest` | `cargo test --doc --locked` | **`--all-targets` excludes doctests** |
| `docs` | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items --locked` | broken links fail |
| `release-build` | `cargo build --release --locked` | catches LTO/strip-only breakage |
| `msrv` | stable + declared `rust-version` | after `rust-version` is set (§10) |

Rules:

- Delete the current "Docs sync (catalog snapshot lives in tests)" step. It
  duplicates `cli_smoke` and verifies no docs. The **real** docs gate arrives
  with `sweep rules list` at M3: compare its output against
  `docs/RULES-CATALOG.md`.
- Do not add Windows/macOS runners.
- A `beta`/nightly build runs only in `scheduled.yml` with
  `continue-on-error: true`; it must never make a PR red for an upstream bug.
- Keep the PR gate under ~5 minutes; anything slower belongs in `scheduled.yml`.

---

## 5. `security.yml`: "is anything vulnerable?"

| Check | Tool | Finds | Blocking? |
|---|---|---|---|
| Advisories | `cargo-deny check advisories` | known-vulnerable crates (RustSec DB) | yes |
| Licenses | `cargo-deny check licenses` | unlicensed / incompatible deps | yes |
| Supply chain | `cargo-deny check sources bans` | git deps, unknown registries, wildcards, duplicates | yes (duplicates warn at first) |
| Unused deps | `cargo-machete` | declared-but-unused crates | yes, with documented ignores |
| PR dependency risk | `actions/dependency-review-action` | vulnerable/mislicensed dep introduced by a PR | yes on PRs |
| Our own code | CodeQL (`languages: rust`) | injection, path handling, logic defects | scheduled first; promote later |
| Secrets | GitHub native secret scanning + push protection | committed credentials | settings-level |
| Workflow security | `actionlint` (+ optional `zizmor`) | shell injection, over-broad permissions | yes |
| Typos | `typos` | code/doc typos | yes |
| Repo posture | OpenSSF Scorecard | branch protection, token permissions, pinned actions | weekly report |

CodeQL and Scorecard run from `scheduled.yml`, not `security.yml`; this table is
the security surface, not a single workflow.

**Implementation note (M1.5, actionlint placement):** `actionlint` runs in
`security.yml` (workflow hygiene is a supply-chain concern), **not** in
`packaging.yml`; the latter contains only PKGBUILD shellcheck + the M7
validation placeholder. This resolves the ambiguous ROADMAP parenthetical.
`zizmor` remains optional and was skipped at M1.5. The `dependency-review`
action has no floating `v4`/`v5` major tag upstream, so it is pinned to its
full (GitHub-owned) `v5.0.0` tag.

### 5.1 `deny.toml` policy

- `[advisories]`: RustSec DB; `yanked = "deny"`; unmaintained entries may be
  ignored **only** with a dated justification comment and a follow-up issue.
- `[licenses]`: allowlist, not blanket. Expected first-run entries:
  `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`,
  `Unicode-3.0` (`unicode-ident`, `unicode-width`), and `MPL-2.0`
  (`option-ext`, pulled by `dirs-sys`). `confidence-threshold = 0.9`.
- `[bans]`: `wildcards = "deny"`, `multiple-versions = "warn"` initially;
  tighten to `deny` only after the duplicate list is reviewed.
- `[sources]`: `unknown-registry = "deny"`, `unknown-git = "deny"` (all deps
  must come from crates.io).
- No `[advisories] ignore` entries without a comment naming the crate, the
  advisory, why it does not apply, and the date.

**Implementation note (M1.5):** the first real run surfaced two `unmaintained`
advisories: `fxhash` (RUSTSEC-2025-0057, via `inquire`) and `number_prefix`
(RUSTSEC-2025-0119, via `indicatif`). Both were ignored with dated
justifications and follow-up hooks; neither is a vulnerability advisory.

**Update (R7, 2026-10-20):** the first Dependabot cargo group PR retired both
ignores. `indicatif` 0.18 dropped `number_prefix` and `inquire` 0.9 dropped
`fxhash`, so neither advisory crate is in the graph any more and cargo-deny
flagged the `ignore` entries as dead. `[advisories] ignore` is now empty; the
list stays empty unless a future advisory genuinely reaches the graph. The
`[bans]` duplicate warning is the expected `unicode-width` pair (0.1.x via
indicatif, 0.2.x direct). The config uses cargo-deny 0.20's version-2 format,
which has no `version =` key.

### 5.2 Unused dependencies

`cargo-machete` will flag crates that are declared but not yet used. Expected
planned-but-unused entries at M1.5 (implemented):

| Crate | Used at | Handling |
|---|---|---|
| `inquire` | M2 (checklist) | ignore, comment "M2" |
| `comfy-table` | possibly never (hand-rolled renderer won) | ignored pending M2: decide remove vs keep |
| `clap_mangen` | M7 (man page) | ignore, comment "M7" |
| `clap_complete` | **used since M1** (`sweep completion <shell>`, `src/cli/mod.rs`) | **not** flagged, no ignore needed; the earlier draft of this table was stale |

The ignore list must shrink as milestones land. Any ignore without a milestone
comment is itself a review failure.

**Implementation note (M1.5):** machete ignores live in Cargo.toml under
`[package.metadata.cargo-machete]` (the section key is `cargo-machete` in
machete 0.9.2, not `machete`); `deny.toml` cannot hold them. This table stays
the human-readable source of truth.

### 5.3 Scheduled runs

`security.yml` runs on PRs, a weekly cron, and `workflow_dispatch`:

- full deny checks (advisories, licenses, bans, sources); new CVEs appear
  without a single commit;
- `cargo-machete`, `actionlint`, `typos`;
- `dependency-review` on PRs only.

`scheduled.yml` runs weekly (and on dispatch):

- CodeQL for Rust (verify preview status on the GitHub side before making it
  blocking);
- OpenSSF Scorecard (`security-events: write`, `id-token: write`);
- beta-toolchain build/tests with `continue-on-error: true`.

---

## 6. `release.yml`: tag-gated publishing

### 6.1 Trigger and dry-run

```yaml
on:
  push:
    tags: ['v*']
  workflow_dispatch:
    inputs:
      dry_run:
        description: "Build + attest, do not publish"
        type: boolean
        default: true
```

Nothing runs on ordinary pushes. A dry run exercises the full matrix, produces
artifacts as workflow artifacts, and skips `gh release create`. On a dry run the
tag/version preflight checks are skipped; only the build and attest steps run.

### 6.2 Jobs

| Job | What it does |
|---|---|
| `preflight` | Tag equals `Cargo.toml` version; `CHANGELOG.md` has a section for it; `Cargo.lock` committed; tag is on `main`; no dirty generated files. |
| `build` | Matrix below; `cargo auditable build --release --locked` so shipped binaries carry dependency metadata. |
| `package` | `sweep-v<ver>-<target>.tar.gz` containing `sweep`, `LICENSE`, `README.md`, `CHANGELOG.md`; sibling `.sha256` per archive plus one aggregate `checksums.sha256`. Also a `sweep-v<ver>-src.tar.gz` (source + `Cargo.lock`, `LICENSE`, README, CHANGELOG) for the source AUR package. M7 adds man page, completions, systemd units. |
| `sbom` | `cargo-cyclonedx` → `sweep-v<ver>.cdx.json`, attached to the release. |
| `attest` | `actions/attest-build-provenance` for every archive (SLSA); `gh attestation verify` must succeed. |
| `publish` | `gh release create --draft` with notes from `CHANGELOG.md`; attach everything; manual publish. |
| `audit-bin` | `cargo audit bin` on each shipped binary (audits what was built, not just the lockfile). |
| `aur` | Optional, gated: regenerate `sweep-bin` checksums and open a PR in this repo. Actual AUR push stays manual (§7.1). |

Source tarball creation: `git archive --prefix=sweep-v<ver>/ -o
sweep-v<ver>-src.tar.gz HEAD`. It must include the tracked `Cargo.lock` and
`LICENSE`; every archive (source included) gets a sibling `.sha256`, and the
job also writes one aggregate `checksums.sha256` covering all archives.

### 6.3 Build matrix

| Target | Runner | Notes |
|---|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04` | baseline; glibc floor comes from 24.04, musl is the portable artifact |
| `x86_64-unknown-linux-musl` | `ubuntu-24.04` + `musl-tools` | static; the `sweep-bin` artifact |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | native arm runner, free for public repos |
| `aarch64-unknown-linux-musl` | optional (`cross`/`zigbuild`) | only if demand appears |

### 6.4 Permissions and safety

- `publish`: `contents: write`; `attest`: `id-token: write` +
  `attestations: write`. Nothing else elevates.
- `environment: release` with a required reviewer (recommended) so a tag push
  alone cannot publish.
- `concurrency: { group: release, cancel-in-progress: false }`.
- Never rebuild the same tag. A bad release is fixed by a new patch version.

### 6.5 Release runbook (M7)

1. Bump `version` in `Cargo.toml`; add the `CHANGELOG.md` section; run the full
   local DoD (`fmt`, `clippy`, `test`, `test --doc`, `build --release`).
2. Commit, push, wait for `ci.yml` + `security.yml` green.
3. `git tag -a v0.1.0 -m "v0.1.0" && git push origin v0.1.0`.
4. Watch `release.yml`: preflight, matrix, checksums, SBOM, attestation.
5. Verify locally: `sha256sum -c checksums.sha256`, `gh attestation verify`,
   `cargo audit bin`.
6. Publish the draft release.
7. Update AUR `sweep-bin` (checksum + `pkgver`), then `sweep` (the source
   tarball changes every release); build both in a clean chroot before pushing.
8. If broken: yank/delete the draft, fix, tag a new patch version.

---

## 7. Distribution

### 7.1 AUR: primary channel

Two packages, in order. The AUR repo name **must** equal `pkgname`; each is a
separate git repo.

| Package | Source | When |
|---|---|---|
| `sweep-bin` | static musl tarball from the GitHub release | first |
| `sweep` | release source tarball, built with the Arch toolchain | after `-bin` is stable |

PKGBUILD requirements:

- `arch=('x86_64')` only. AUR is officially x86_64; aarch64 belongs to Arch
  Linux ARM and is served by GitHub release binaries, not the AUR `arch` array.
- `license=('MIT')`, `url`, `pkgdesc`. `sweep-bin` sets
  `provides=('sweep')` + `conflicts=('sweep')`; `sweep` sets
  `conflicts=('sweep-bin')` (both install `/usr/bin/sweep`).
- `sweep-bin`: `source_x86_64=("$url/releases/download/v$pkgver/sweep-v$pkgver-x86_64-unknown-linux-musl.tar.gz")`,
  `sha256sums_x86_64=(...)`, `depends=()` (static binary).
- `sweep` (source): `depends=('gcc-libs')`, `makedepends=('cargo' 'rust')`,
  `source=("sweep-v$pkgver-src.tar.gz::$url/releases/download/v$pkgver/sweep-v$pkgver-src.tar.gz")`;
  `build()`/`check()` `cd "$srcdir/sweep-v$pkgver"`. `build()` runs
  `cargo build --release --locked`, **not** `--frozen` (`--frozen` implies
  offline and fails without a populated registry cache). `check()` runs
  `cargo test --release --locked`.
- `package()` installs `usr/bin/sweep`; from M7 also
  `usr/share/man/man1/sweep.1`, `usr/share/bash-completion/completions/sweep`,
  `usr/share/zsh/site-functions/_sweep`, `usr/share/fish/vendor_completions.d/sweep.fish`,
  and the systemd units under `usr/lib/systemd/`.
- `.SRCINFO` committed and current (`makepkg --printsrcinfo`). Do not hand-edit.
- Use a **release tarball**, not `archive/refs/tags/...`; GitHub's auto-generated
  archives are not guaranteed stable for package sources.

Publishing: first upload is manual (`ssh://aur@aur.archlinux.org/sweep-bin.git`).
Automation is optional and gated on an `AUR_SSH_PRIVATE_KEY` secret; it must
never run on forks.

### 7.2 Static binaries

- gnu + musl, x86_64 (+ aarch64 gnu on the arm runner).
- Archives contain the binary, `LICENSE`, `README.md`, `CHANGELOG.md`; man page,
  completions, and units join at M7. Binary archives extract to
  `sweep-v<ver>-<target>/`; the source tarball extracts to `sweep-v<ver>/`.
- Every archive gets a `.sha256` and a build-provenance attestation.
- `cargo auditable` metadata must survive stripping. ~~The current release
  profile sets `strip = true`~~; **M1.5 changed it to `strip = "debuginfo"`**
  so the audit section survives; the release job's `audit-bin` step
  (`cargo audit bin` on the musl binary) proves it on every tag.

### 7.3 Flatpak: DEFERRED (decision D11)

**Not packaged for 0.1, and not planned before 1.0.** This is a product decision,
not a scheduling one:

- Flatpak gives the app a **private `/tmp`**; scanning the host's `/tmp` is wrong
  or requires `--filesystem=/tmp`.
- There is **no sudo** inside the sandbox. System targets (`/var/cache`,
  systemd journal vacuuming, pacman cache, rootful docker/podman) are
  unreachable by design.
- `$HOME` access needs `--filesystem=home`, which Flathub reviews closely; a
  tool that can delete anything in home will draw exactly that scrutiny.
- The usual workaround (`--filesystem=host` + `org.freedesktop.Flatpak` host
  command execution) is precisely the broad permission a safety-first cleaner
  should refuse to normalize.

A home-only Flatpak build (cache, thumbnails, trash) is technically possible
with vendored crates + `org.freedesktop.Sdk.Extension.rust-stable`, but it is a
different, lesser product under the same name. Revisit post-1.0 only if there is
real demand; if reopened, the plan is: app ID `io.github.mhdztr.sweep`,
`flatpak-cargo-generator` for `cargo-sources.json`, metainfo + icon (required
even for CLI on Flathub), `finish-args` limited to `--filesystem=home`, and no
CI until the manifest exists.

### 7.4 crates.io: deferred

`cargo install --path .` remains documented for local builds. Publishing to
crates.io is not a distribution strategy for a root-capable cleaner and adds
release surface; revisit only on demand.

### 7.5 Other channels: post-1.0

`.deb`/`.rpm` via `nfpm`, and nothing else. AppImage adds nothing for a CLI.

### 7.6 Planned packaging layout

```
packaging/
├── aur/
│   ├── sweep-bin/{PKGBUILD,.SRCINFO}
│   └── sweep/{PKGBUILD,.SRCINFO}
```

`docs/RELEASING` is not a separate file: the runbook is §6.5 here. If the
release process grows enough to need its own page, split it then.
`packaging/README.md` already records this layout; only the recipes land at M7.

---

## 8. Repository governance

One-time GitHub settings (not YAML):

- Dependabot alerts + security updates; `.github/dependabot.yml` with weekly,
  grouped updates for `cargo` and `github-actions`.
- Secret scanning + **push protection**; private vulnerability reporting.
- **Dependency graph must be enabled** (Settings > Security > Advanced
  Security) before `dependency-review` can run. Without it every PR fails
  `dependency-review` with "Dependency review is not supported on this
  repository": a settings gap, not a code failure. Enable it first, then
  enable Dependabot alerts; the two share the same graph.
- Branch protection on `main`: require `fmt`, `clippy`, `test`, `doctest`,
  `docs`, `release-build`, and `deny`; require a PR; dismiss stale approvals;
  block force-pushes.
- Ruleset protecting `v*` tags from deletion/moves.
- Note the workflow tradeoff: required checks mean working through PRs rather
  than pushing directly to `main`. This is the recommended setting for a
  safety-first project, but it is reversible in repository settings.

Repository files for the foundation (M1.5):

| File | Purpose |
|---|---|
| `SECURITY.md` | disclosure policy, supported versions, private reporting link |
| `CHANGELOG.md` | Keep a Changelog; release notes are extracted from it |
| `deny.toml` | policy in §5.1 |
| `.github/dependabot.yml` | grouped weekly updates |
| `.github/workflows/*.yml` | §3 (`packaging.yml` is a skeleton until M7) |
| `packaging/README.md` | layout + when each AUR package lands (§7.6); already added |

---

## 9. Phases

| Phase | When | Work |
|---|---|---|
| **0: foundation** | now, before M2 | Infra only, no product code: CI restructure, `deny.toml` + security workflow, Dependabot, repo settings, `SECURITY.md`, `CHANGELOG.md`, Cargo metadata, `release.yml` scaffold, `packaging/` layout + `packaging.yml` skeleton (PKGBUILDs land at M7). |
| **1: while M2/M3 land** | during | Keep the gate green; real docs-sync check at M3 (`rules list` snapshot); coverage report (not a gate); CodeQL/Scorecard scheduled; shrink machete ignores as deps get used. |
| **2: ship (M6/M7)** | before 0.1.0 | Final `release.yml` (SBOM, provenance, `cargo auditable`), PKGBUILDs, `packaging.yml` green, tag `v0.1.0`, publish `sweep-bin` then `sweep`. |
| **3: post-1.0** | later | Flatpak reconsideration (home-only), `.deb`/`.rpm`, crates.io only if demanded. |

---

## 10. Cargo metadata prerequisites

Add before any release or packaging work:

```toml
[package]
repository = "https://github.com/mhdztr/sweep"
homepage   = "https://github.com/mhdztr/sweep"
rust-version = "<discover with cargo msrv>"   # edition 2024 floor is 1.85
```

`rust-version` must be discovered, not guessed: the dependency graph decides the
real floor. The `msrv` CI job then pins it.

**Implementation note (M1.5):** `rust-version = "1.88"` was discovered by
toolchain bisection (`cargo +<N> build --locked`): 1.85 (the edition-2024
floor) and 1.86/1.87 fail on `comfy-table` (let chains, stabilized in 1.88);
1.88 is the first green build. `cargo msrv find` itself could not run on the
discovery host (its dist-index download was blocked there), hence the
documented fallback. The `msrv` job in `ci.yml` pins `1.88.0` and is
deliberately **not** a required check (§8).

---

## 11. Verification (definition of done)

**M1.5 (foundation):**

- [ ] New workflows pass on a PR (not a direct push).
- [x] `actionlint` clean on every workflow file. (Locally verified on
      2026-10-20; the CI job re-verifies on every PR.)
- [x] `cargo deny check` green with a reviewed allowlist; every ignore has a
      crate, advisory, reason, and date. (The two `unmaintained` ignores are
      documented in `deny.toml` and §5.1.)
- [x] `cargo machete` green with milestone-commented ignores.
- [ ] `cargo test --doc` runs in CI and passes. (Passes locally; CI
      verification pending the first PR.)
- [ ] `cargo build --release --locked` runs in CI. (Passes locally; CI
      verification pending the first PR.)
- [ ] `release.yml` dry run produces archives, `.sha256`, SBOM, and an
      attestation, and publishes nothing.
- [ ] `packaging.yml` dispatch run is green and skips cleanly when no PKGBUILDs
      exist (the AUR job activates at M7).
- [ ] Branch protection + tag protection active.
- [ ] Dependabot opens its first grouped PR.

**M7 (release):**

- [ ] Tag `v0.1.0` produces a draft release with target archives, a source
      tarball, `.sha256` files, and an SBOM.
- [ ] `sha256sum -c checksums.sha256` and `gh attestation verify` succeed on a
      clean machine.
- [ ] `cargo audit bin` reports no advisories for the shipped binaries.
- [ ] `packaging/aur/sweep-bin` and `packaging/aur/sweep` build in
      `archlinux:base-devel` as a non-root user; `namcap` clean; `.SRCINFO`
      current.
- [ ] `sweep-bin` installs from the AUR on a fresh Arch VM and `sweep doctor`
      is clean.
- [ ] Source `sweep` package builds in a clean chroot.

**Out of scope for CI (must stay manual):** Arch-specific behavior (pacman,
systemd, btrfs/snapper), sudo flows, and the fresh-VM walkthrough. The M7 manual
matrix remains the real release gate; CI cannot substitute for it.

---

## 12. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| `cargo deny` noise on first run | red CI ignored | land with a reviewed allowlist; duplicates warn initially |
| CodeQL Rust preview instability | flaky required check | scheduled first, promote only when stable |
| Arm runner availability/quota | release matrix fails | `workflow_dispatch` dry run before every tag; gnu arm is optional |
| AUR PKGBUILD rot | broken installs | `packaging.yml` container build + `.SRCINFO` diff |
| Action supply chain | CI compromise | SHA-pin third-party actions; Dependabot updates them |
| CI cannot test Arch/sudo | false confidence | documented out-of-scope list; M7 manual VM matrix is the gate |
| Too many jobs | slow PRs | concurrency cancellation, timeouts, slow checks moved to scheduled |
| Machete ignores hide real junk | dependency bloat | every ignore names a milestone; list reviewed each milestone |
| Release automation goes wrong | bad artifacts | draft releases + reviewer environment + never reuse a tag |

---

## 13. References

- GitHub Actions hardening: <https://docs.github.com/en/actions/security-guides/security-hardening-for-github-actions>
- `cargo-deny`: <https://embarkstudios.github.io/cargo-deny/>
- RustSec advisory database: <https://rustsec.org/>
- AUR submission guidelines: <https://wiki.archlinux.org/title/AUR_submission_guidelines>
- `makepkg` / PKGBUILD: <https://wiki.archlinux.org/title/PKGBUILD>
- SLSA provenance: <https://slsa.dev/>
- OpenSSF Scorecard: <https://github.com/ossf/scorecard>
