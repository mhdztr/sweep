# Security policy

## Supported versions

`sweep` is pre-1.0 (see `ROADMAP.md`). Only the latest commit on `main` is
supported; there are no backports.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | pre-alpha, latest commit on `main` only |

## Reporting a vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Use GitHub's private vulnerability reporting instead:

<https://github.com/mhdztr/sweep/security/advisories/new>

Include, when known:

- the affected component (`scan`, rules, path handling, the planned `exec`
  pipeline, CI/packaging);
- a reproduction (command line, config, filesystem layout);
- impact assessment in your own words (what could a malformed plan, symlink,
  or config do?);
- any proposed fix.

If private reporting is unavailable on the repository, open an issue asking
for contact details only — never include vulnerability details in it.

## Response expectations

- Acknowledgement: within **7 days**.
- Triage and status update: within **30 days**.
- Fix or mitigation plan for confirmed issues: targeted at the next milestone
  release; critical issues may get an out-of-order commit on `main`.
- Credit: reporters are credited in the advisory unless they prefer otherwise.

These are best-effort targets of a single-maintainer project, not a contract.

## Security-relevant design commitments

These invariants are enforced by tests and review (see `AGENTS.md`,
`docs/SAFETY.md`, `docs/ARCHITECTURE.md`):

- **No network, ever.** `sweep` makes no outbound connections at runtime, in
  any mode, including tests and CI jobs that run it. External programs are
  invoked as argv arrays only (never through a shell).
- **`scan` and `--dry-run` mutate nothing.** The execution path re-validates
  every target against the never-touch list independently of the scan plan.
- **`--yes` never bypasses destructive gates**; only the explicit
  `--i-know-what-im-doing` flag does, and destructive capability always ships
  with its gate in the same change.
- **No privilege escalation without an explicit prompt**: the batched sudo
  pass asks first and re-validates before touching anything.

A break of any of these is a security bug; report it privately.

## Disclosure process

1. Report received privately; acknowledged and triaged.
2. Fix developed on `main` (optionally behind a private fork advisory for
   coordinated disclosure).
3. Advisory published with the fix; `CHANGELOG.md` updated.
4. For released artifacts: a new version tag triggers the release workflow;
   old tags are never rebuilt or reused.
