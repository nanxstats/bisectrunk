mod common;

use std::fs;

use predicates::prelude::*;

fn bisect_command(
    fixture: &common::FixtureRepo,
    run_dir: &std::path::Path,
    cache_dir: &std::path::Path,
    jobs: usize,
    good: &str,
) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("bisectrunk"));
    command.args([
        "bisect",
        "--repo",
        fixture.path.to_str().expect("UTF-8 fixture path"),
        "--good",
        good,
        "--bad",
        fixture.shas.last().expect("last fixture commit"),
        "--run",
        common::run_hook(),
        "--jobs",
        &jobs.to_string(),
        "--format",
        "plain",
        "--run-dir",
        run_dir.to_str().expect("UTF-8 run dir"),
        "--cache-dir",
        cache_dir.to_str().expect("UTF-8 cache dir"),
    ]);
    command
}

#[test]
fn bisect_finds_exact_first_bad_with_one_or_eight_jobs() {
    let fixture = common::FixtureBuilder::new(40).flip_at(23).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    for jobs in [1, 8] {
        let run_dir = temp.path().join(format!("bisect-{jobs}"));
        let cache_dir = temp.path().join(format!("cache-{jobs}"));
        bisect_command(&fixture, &run_dir, &cache_dir, jobs, &fixture.shas[0])
            .assert()
            .success()
            .stdout(predicate::str::contains(format!(
                "first bad commit: {}",
                fixture.first_bad
            )));
        let report = fs::read_to_string(run_dir.join("report.md")).expect("read report");
        assert!(report.contains(&format!("First bad commit: `{}`", fixture.first_bad)));
    }
}

#[test]
fn bisect_reports_skip_candidate_set_adjacent_to_transition() {
    let fixture = common::FixtureBuilder::new(40)
        .flip_at(20)
        .broken_at([19, 20])
        .build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let run_dir = temp.path().join("bisect-skips");
    bisect_command(
        &fixture,
        &run_dir,
        &temp.path().join("cache"),
        8,
        &fixture.shas[0],
    )
    .assert()
    .code(2)
    .stdout(predicate::str::contains("candidate set:"));
    let report = fs::read_to_string(run_dir.join("report.md")).expect("read report");
    assert!(report.contains(&fixture.shas[19]));
    assert!(report.contains(&fixture.shas[20]));
    assert!(report.contains(&fixture.shas[21]));
}

#[test]
fn endpoint_contradiction_exits_three_with_precise_message() {
    let fixture = common::FixtureBuilder::new(8).flip_at(3).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let run_dir = temp.path().join("endpoint-failure");
    bisect_command(
        &fixture,
        &run_dir,
        &temp.path().join("cache"),
        2,
        &fixture.shas[4],
    )
    .assert()
    .code(3)
    .stdout(predicate::str::contains("endpoint verification failed"))
    .stdout(predicate::str::contains(&fixture.shas[4]));
}
