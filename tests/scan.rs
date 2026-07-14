mod common;

use std::fs;

use predicates::prelude::*;

#[test]
fn scan_detects_the_planted_transition_and_writes_a_ledger() {
    let fixture = common::FixtureBuilder::new(12).flip_at(6).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let run_dir = temp.path().join("scan-run");
    let cache_dir = temp.path().join("cache");
    let range = format!("{}..{}", fixture.shas[0], fixture.shas[11]);
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("bisectrunk"));
    command.args([
        "scan",
        "--repo",
        fixture.path.to_str().expect("UTF-8 fixture path"),
        "--range",
        &range,
        "--run",
        common::run_hook(),
        "--jobs",
        "4",
        "--format",
        "plain",
        "--run-dir",
        run_dir.to_str().expect("UTF-8 run dir"),
        "--cache-dir",
        cache_dir.to_str().expect("UTF-8 cache dir"),
    ]);
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("report:"));

    let markdown = fs::read_to_string(run_dir.join("report.md")).expect("read Markdown report");
    assert!(markdown.contains(&fixture.first_bad[..12]));
    assert!(markdown.contains("good →"));
    assert!(markdown.contains("bad"));
    let state: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(run_dir.join("state.json")).expect("read state ledger"),
    )
    .expect("parse state ledger");
    assert_eq!(state["complete"], true);
    assert_eq!(
        state["evaluations"]
            .as_object()
            .expect("evaluation object")
            .len(),
        11
    );
}

#[test]
fn report_rerenders_without_evaluating_again() {
    let fixture = common::FixtureBuilder::new(3).flip_at(2).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let run_dir = temp.path().join("scan-run");
    let cache_dir = temp.path().join("cache");
    let range = format!("{}..{}", fixture.shas[0], fixture.shas[2]);
    let binary = assert_cmd::cargo::cargo_bin!("bisectrunk");
    assert_cmd::Command::new(binary)
        .args([
            "scan",
            "--repo",
            fixture.path.to_str().expect("UTF-8 fixture path"),
            "--range",
            &range,
            "--run",
            common::run_hook(),
            "--run-dir",
            run_dir.to_str().expect("UTF-8 run dir"),
            "--cache-dir",
            cache_dir.to_str().expect("UTF-8 cache dir"),
        ])
        .assert()
        .success();
    let before = fs::read_to_string(run_dir.join("state.json")).expect("read state before");
    fs::remove_file(run_dir.join("report.md")).expect("remove report");
    assert_cmd::Command::new(binary)
        .arg("report")
        .arg(&run_dir)
        .assert()
        .success();
    let after = fs::read_to_string(run_dir.join("state.json")).expect("read state after");
    assert_eq!(before, after);
    assert!(run_dir.join("report.md").is_file());
}
