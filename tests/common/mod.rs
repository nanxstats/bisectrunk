#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use xshell::{Shell, cmd};

pub struct FixtureRepo {
    _temp: TempDir,
    pub path: PathBuf,
    pub shas: Vec<String>,
    pub first_bad: String,
}

pub struct FixtureBuilder {
    commits: usize,
    flip_at: usize,
    broken: Vec<usize>,
}

impl FixtureBuilder {
    pub fn new(commits: usize) -> Self {
        Self {
            commits,
            flip_at: commits / 2,
            broken: Vec::new(),
        }
    }

    pub fn flip_at(mut self, index: usize) -> Self {
        self.flip_at = index;
        self
    }

    pub fn broken_at(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        self.broken = indices.into_iter().collect();
        self
    }

    pub fn build(self) -> FixtureRepo {
        assert!(self.commits > 0);
        assert!(self.flip_at < self.commits);
        let temp = tempfile::tempdir().expect("create fixture tempdir");
        let path = temp.path().join("subject");
        let shell = Shell::new().expect("create fixture shell");
        shell.set_var("GIT_AUTHOR_NAME", "Bisectrunk Fixture");
        shell.set_var("GIT_AUTHOR_EMAIL", "fixture@example.invalid");
        shell.set_var("GIT_COMMITTER_NAME", "Bisectrunk Fixture");
        shell.set_var("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
        shell.set_var("GIT_AUTHOR_DATE", "2000-01-01T00:00:00+0000");
        shell.set_var("GIT_COMMITTER_DATE", "2000-01-01T00:00:00+0000");
        cmd!(shell, "git init --quiet {path}")
            .run()
            .expect("initialize fixture repository");
        let mut shas = Vec::with_capacity(self.commits);
        for index in 0..self.commits {
            let marker = if index >= self.flip_at {
                "bad\n"
            } else {
                "good\n"
            };
            fs::write(path.join("marker.txt"), marker).expect("write marker");
            let broken_path = path.join("BROKEN");
            if self.broken.contains(&index) {
                fs::write(&broken_path, "broken\n").expect("write broken marker");
            } else if broken_path.exists() {
                fs::remove_file(&broken_path).expect("remove broken marker");
            }
            fs::write(path.join("counter.txt"), format!("{index}\n")).expect("write counter");
            cmd!(shell, "git -C {path} add -A")
                .run()
                .expect("stage fixture commit");
            let message = format!("fixture commit {index:04}");
            cmd!(shell, "git -C {path} commit --quiet -m {message}")
                .run()
                .expect("create fixture commit");
            let sha = cmd!(shell, "git -C {path} rev-parse HEAD")
                .read()
                .expect("read fixture commit SHA");
            shas.push(sha);
        }
        let first_bad = shas[self.flip_at].clone();
        FixtureRepo {
            _temp: temp,
            path,
            shas,
            first_bad,
        }
    }
}

pub fn run_hook() -> &'static str {
    "if [ -f \"$BISECTRUNK_WORKTREE/BROKEN\" ]; then exit 125; fi; test \"$(cat \"$BISECTRUNK_WORKTREE/marker.txt\")\" = good"
}

pub fn command_for(fixture: &FixtureRepo, run_dir: &Path, cache_dir: &Path) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("bisectrunk"));
    command.args([
        "run",
        "--repo",
        fixture.path.to_str().expect("UTF-8 fixture path"),
        "--at",
        &fixture.shas[0],
        "--run-dir",
        run_dir.to_str().expect("UTF-8 run dir"),
        "--cache-dir",
        cache_dir.to_str().expect("UTF-8 cache dir"),
    ]);
    command
}
