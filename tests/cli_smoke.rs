//! CLI surface smoke tests (M0): parsing, help/version, exit codes, stubs.
//! Spec: docs/CLI-UX.md — messages here are copy-locked.
//!
//! Env-mutating tests live in `env_guard.rs` / `sandbox.rs` (own processes);
//! this file only spawns child processes and must stay env-clean.

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt as _;

fn sweep() -> Command {
    Command::cargo_bin("sweep").expect("cargo_bin")
}

#[test]
fn version_reports_package_version() {
    sweep()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains("sweep 0.1.0"));
}

#[test]
fn help_lists_every_documented_command() {
    let out = String::from_utf8(
        sweep()
            .arg("--help")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .expect("utf8");

    for cmd in [
        "scan",
        "clean",
        "top",
        "history",
        "doctor",
        "rules",
        "config",
        "completion",
    ] {
        assert!(out.contains(cmd), "help must document `{cmd}`:\n{out}");
    }
    for flag in [
        "--dry-run",
        "--json",
        "--keep-days",
        "--trash",
        "--yes",
        "--preset",
    ] {
        assert!(out.contains(flag), "help must document `{flag}`:\n{out}");
    }
}

#[test]
fn no_args_defaults_to_clean_flow() {
    // Until M2 lands, default dispatch hits the clean stub: exit 1 + copy.
    sweep()
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains(
            "`sweep clean` is not implemented yet",
        ));
}

#[test]
fn stub_commands_exit_1_with_milestone_hint() {
    for (args, milestone) in [
        (vec!["scan"], "M1"),
        (vec!["doctor"], "M5"),
        (vec!["history"], "M4"),
        (vec!["rules", "list"], "M3"),
        (vec!["config", "path"], "M6"),
    ] {
        sweep().args(&args).assert().failure().code(1).stderr(
            predicates::str::contains("not implemented yet")
                .and(predicates::str::contains(milestone)),
        );
    }
}

#[test]
fn completion_renders_bash_script() {
    sweep()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicates::str::contains("sweep"));
}

#[test]
fn hidden_internal_pass_flag_is_hidden_but_parses() {
    let help = String::from_utf8(
        sweep()
            .arg("--help")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .expect("utf8");
    assert!(!help.contains("internal-system-pass"), "must stay hidden");

    sweep()
        .args(["internal-system-pass", "/tmp/nonexistent-plan.json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("M5"));
}

#[test]
fn destructive_bypass_flag_parses_and_is_hidden() {
    let help = String::from_utf8(
        sweep()
            .arg("--help")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .expect("utf8");
    assert!(!help.contains("--i-know-what-im-doing"));

    // Parses globally even after a subcommand; still hits the stub (exit 1).
    sweep()
        .args(["scan", "--i-know-what-im-doing", "--dry-run"])
        .assert()
        .failure()
        .code(1);
}
