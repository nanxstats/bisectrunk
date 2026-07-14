mod common;

use predicates::prelude::*;

#[test]
fn run_classifies_all_exit_protocol_outcomes() {
    let fixture = common::FixtureBuilder::new(1).flip_at(0).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let cases = [
        (0, "good", 0),
        (1, "bad", 0),
        (125, "skip", 2),
        (130, "abort", 4),
    ];
    for (hook_code, classification, process_code) in cases {
        let run_dir = temp.path().join(format!("run-{hook_code}"));
        let mut command = common::command_for(&fixture, &run_dir, &temp.path().join("cache"));
        command.args(["--run", &format!("exit {hook_code}")]);
        command
            .assert()
            .code(process_code)
            .stdout(predicate::str::contains(format!(
                " {classification} (exit {hook_code})"
            )));
        let log = run_dir.join("logs").join(&fixture.shas[0]).join("run.log");
        assert!(log.is_file(), "missing hook log {}", log.display());
    }
}

#[test]
fn run_uses_worktree_contract_and_setup_policy() {
    let fixture = common::FixtureBuilder::new(1).flip_at(0).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let run_dir = temp.path().join("run");
    let mut command = common::command_for(&fixture, &run_dir, &temp.path().join("cache"));
    command.args(["--setup", "exit 1", "--run", "exit 0"]);
    command
        .assert()
        .code(2)
        .stdout(predicate::str::contains(" skip (exit 1)"));

    let bad_dir = temp.path().join("bad-setup");
    let mut command = common::command_for(&fixture, &bad_dir, &temp.path().join("cache"));
    command.args([
        "--setup",
        "exit 1",
        "--setup-failure",
        "bad",
        "--run",
        "exit 0",
    ]);
    command
        .assert()
        .success()
        .stdout(predicate::str::contains(" bad (exit 1)"));
}
