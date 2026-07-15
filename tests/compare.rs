mod common;

use std::fs;

use predicates::prelude::*;

#[test]
fn compare_bisect_finds_transition_and_reports_unified_diff() {
    let fixture = common::FixtureBuilder::new(16).flip_at(9).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let baseline = temp.path().join("baseline.txt");
    fs::write(&baseline, "good\n").expect("write baseline");
    let run_dir = temp.path().join("compare-bisect");
    let cache_dir = temp.path().join("cache");
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("bisectrunk"));
    command.args([
        "bisect",
        "--repo",
        fixture.path.to_str().expect("UTF-8 fixture path"),
        "--good",
        &fixture.shas[0],
        "--bad",
        fixture.shas.last().expect("last fixture commit"),
        "--run",
        "cp \"$BISECTRUNK_WORKTREE/marker.txt\" \"$BISECTRUNK_OUT/result.txt\"",
        "--oracle",
        "compare",
        "--baseline",
        baseline.to_str().expect("UTF-8 baseline path"),
        "--artifact",
        "result.txt",
        "--jobs",
        "4",
        "--format",
        "plain",
        "--run-dir",
        run_dir.to_str().expect("UTF-8 run dir"),
        "--cache-dir",
        cache_dir.to_str().expect("UTF-8 cache path"),
    ]);
    command
        .assert()
        .success()
        .stdout(predicate::str::contains(&fixture.first_bad));
    let report = fs::read_to_string(run_dir.join("report.md")).expect("read report");
    assert!(report.contains("## Artifact differences"));
    assert!(report.contains("-good"));
    assert!(report.contains("+bad"));
}

#[test]
fn custom_compare_receives_artifact_paths_and_missing_artifact_skips() {
    let fixture = common::FixtureBuilder::new(2).flip_at(1).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let baseline = temp.path().join("baseline.txt");
    fs::write(&baseline, "good\n").expect("write baseline");
    let binary = assert_cmd::cargo::cargo_bin!("bisectrunk");
    let cache_dir = temp.path().join("cache");
    let custom_dir = temp.path().join("custom");
    let missing_dir = temp.path().join("missing");
    let common_args = [
        "--repo",
        fixture.path.to_str().expect("UTF-8 fixture path"),
        "--at",
        &fixture.shas[1],
        "--oracle",
        "compare",
        "--baseline",
        baseline.to_str().expect("UTF-8 baseline path"),
        "--artifact",
        "result.txt",
        "--cache-dir",
        cache_dir.to_str().expect("UTF-8 cache path"),
    ];
    assert_cmd::Command::new(binary)
        .arg("run")
        .args(common_args)
        .args([
            "--run",
            "cp \"$BISECTRUNK_WORKTREE/marker.txt\" \"$BISECTRUNK_OUT/result.txt\"",
            "--compare",
            "cmp \"$BISECTRUNK_BASELINE\" \"$BISECTRUNK_CANDIDATE\"",
            "--run-dir",
            custom_dir.to_str().expect("UTF-8 run path"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(" bad (exit 1,"));

    assert_cmd::Command::new(binary)
        .arg("run")
        .args(common_args)
        .args([
            "--run",
            "exit 0",
            "--run-dir",
            missing_dir.to_str().expect("UTF-8 run path"),
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains(" skip (exit 125,"));
}
