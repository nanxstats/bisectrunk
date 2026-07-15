mod common;

use std::fs;
use std::process::Stdio;
use std::time::Duration;

use predicates::prelude::*;

#[test]
fn retries_can_recover_a_flaky_bad_result_and_timeout_skips() {
    let fixture = common::FixtureBuilder::new(1).flip_at(0).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let counter = temp.path().join("counter");
    let mut retry = common::command_for(
        &fixture,
        &temp.path().join("retry-run"),
        &temp.path().join("cache"),
    );
    retry.args([
        "--run",
        "n=$(cat \"$COUNTER\" 2>/dev/null || echo 0); n=$((n + 1)); echo \"$n\" > \"$COUNTER\"; test \"$n\" -ge 2",
        "--env",
        &format!("COUNTER={}", counter.display()),
        "--retries",
        "1",
    ]);
    retry
        .assert()
        .success()
        .stdout(predicate::str::contains(" good (exit 0,"));
    assert_eq!(
        fs::read_to_string(&counter)
            .expect("read retry counter")
            .trim(),
        "2"
    );

    let mut timeout = common::command_for(
        &fixture,
        &temp.path().join("timeout-run"),
        &temp.path().join("timeout-cache"),
    );
    timeout.args(["--run", "sleep 0.2", "--timeout", "20ms"]);
    timeout
        .assert()
        .code(2)
        .stdout(predicate::str::contains(" skip (exit"));
}

#[test]
fn pin_setup_is_installed_once_and_exported_to_hooks() {
    let fixture = common::FixtureBuilder::new(1).flip_at(0).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let config_path = temp.path().join("pins.toml");
    fs::write(
        &config_path,
        format!(
            "[[pins]]\nrepo = {:?}\nrev = {:?}\nsetup = 'cp \"$BISECTRUNK_PIN_WORKTREE/marker.txt\" \"$BISECTRUNK_PIN_ENV/pinned.txt\"'\n",
            fixture.path.to_string_lossy(),
            fixture.shas[0]
        ),
    )
    .expect("write pin config");
    let cache_dir = temp.path().join("cache");
    for index in 0..2 {
        let run_dir = temp.path().join(format!("pin-run-{index}"));
        let mut command = common::command_for(&fixture, &run_dir, &cache_dir);
        command.args([
            "--config",
            config_path.to_str().expect("UTF-8 config path"),
            "--run",
            "test -f \"$BISECTRUNK_PIN_ENVS/pinned.txt\"",
        ]);
        command.assert().success();
        if index == 0 {
            assert!(run_dir.join("logs/pins/0-24b698e81b4a/setup.log").is_file());
        } else {
            assert!(!run_dir.join("logs/pins").exists());
        }
    }
}

#[test]
fn json_format_emits_only_json_lines_with_required_events() {
    let fixture = common::FixtureBuilder::new(10).flip_at(6).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let run_dir = temp.path().join("json-run");
    let cache_dir = temp.path().join("json-cache");
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("bisectrunk"))
        .args([
            "bisect",
            "--repo",
            fixture.path.to_str().expect("UTF-8 fixture path"),
            "--good",
            &fixture.shas[0],
            "--bad",
            fixture.shas.last().expect("last fixture commit"),
            "--run",
            common::run_hook(),
            "--jobs",
            "3",
            "--format",
            "json",
            "--run-dir",
            run_dir.to_str().expect("UTF-8 run path"),
            "--cache-dir",
            cache_dir.to_str().expect("UTF-8 cache path"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("JSON output is UTF-8");
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid JSON line"))
        .filter_map(|value| value["event"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert!(events.iter().any(|event| event == "round_start"));
    assert!(events.iter().any(|event| event == "evaluation"));
    assert!(events.iter().any(|event| event == "conclusion"));
}

#[cfg(unix)]
#[test]
fn interrupt_then_resume_preserves_completed_evaluation_timestamps() {
    let fixture = common::FixtureBuilder::new(14).flip_at(9).build();
    let temp = tempfile::tempdir().expect("create command tempdir");
    let run_dir = temp.path().join("resume-run");
    let cache_dir = temp.path().join("resume-cache");
    let range = format!("{}..{}", fixture.shas[0], fixture.shas[13]);
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("bisectrunk"))
        .args([
            "scan",
            "--repo",
            fixture.path.to_str().expect("UTF-8 fixture path"),
            "--range",
            &range,
            "--run",
            &format!("sleep 0.1; {}", common::run_hook()),
            "--jobs",
            "1",
            "--run-dir",
            run_dir.to_str().expect("UTF-8 run path"),
            "--cache-dir",
            cache_dir.to_str().expect("UTF-8 cache path"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn interruptible scan");
    let state_path = run_dir.join("state.json");
    let mut observed = None;
    for _ in 0..150 {
        if let Ok(contents) = fs::read_to_string(&state_path)
            && let Ok(state) = serde_json::from_str::<serde_json::Value>(&contents)
            && state["evaluations"]
                .as_object()
                .is_some_and(|map| !map.is_empty())
        {
            observed = Some(state);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let before = observed.expect("at least one completed evaluation before interrupt");
    std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    let status = child.wait().expect("wait for interrupted scan");
    assert_eq!(status.code(), Some(2));
    let before_timestamps = completion_timestamps(&before);
    assert!(!before_timestamps.is_empty());

    assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("bisectrunk"))
        .arg("resume")
        .arg(&run_dir)
        .assert()
        .success();
    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read resumed state"))
            .expect("parse resumed state");
    for (sha, timestamp) in before_timestamps {
        assert_eq!(after["evaluations"][sha]["completed_at"], timestamp);
    }
    assert_eq!(after["complete"], true);
}

fn completion_timestamps(state: &serde_json::Value) -> Vec<(String, serde_json::Value)> {
    state["evaluations"]
        .as_object()
        .expect("evaluation map")
        .iter()
        .map(|(sha, evaluation)| (sha.clone(), evaluation["completed_at"].clone()))
        .collect()
}
