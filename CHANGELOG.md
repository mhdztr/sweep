# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **M1.5 — repo hardening + CI/CD foundation (infra only, no `sweep` behavior
  change):**
  - `ci.yml` restructured into `fmt`, `clippy`, `test`, `doctest`, `docs`,
    `release-build`, and `msrv` jobs with `--locked`, timeouts,
    concurrency cancellation, and least-privilege permissions.
  - `security.yml`: `cargo-deny` (advisories, licenses, bans, sources),
    `cargo-machete`, `actionlint`, `typos`, and PR-only
    `dependency-review`; weekly cron + manual dispatch.
  - `scheduled.yml`: weekly CodeQL (Rust), OpenSSF Scorecard (SARIF), and a
    non-blocking beta-toolchain build.
  - `release.yml`: tag-gated (`v*`) build/package/SBOM/attestation scaffold
    with a manual dry-run dispatch that publishes nothing; `audit-bin` job
    proves `cargo auditable` metadata survives stripping.
  - `packaging.yml`: PKGBUILD shellcheck + validation skeleton, skipping
    cleanly until the M7 recipes exist.
  - `.github/dependabot.yml`: grouped weekly `cargo` + `github-actions`
    updates.
  - `deny.toml`: reviewed license allowlist, `yanked = "deny"`, wildcard
    bans, registry/source restrictions (docs/CI-CD.md §5.1).
  - `SECURITY.md` (disclosure policy) and this changelog.
  - `Cargo.toml`: `repository`, `homepage`, `rust-version = "1.88"`
    (discovered), `strip = "debuginfo"` in the release profile so
    `cargo auditable` metadata survives; milestone-commented cargo-machete
    ignores.

Nothing user-visible changed: `sweep` behavior, CLI output, and tests are
identical to the M1 state.

### Notes

- Version `0.1.0` is **unreleased**; this section absorbs changes until the
  first tag (M7).
