//! What rt checks on this machine before it contacts an instance. The instance here is an
//! address nothing answers on, so an error about anything else shows rt stopped first. The
//! review loop against a server is tested where the server is.

use std::process::{Command, Output};

struct Env {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let env = Env {
            home: tempfile::tempdir().unwrap(),
            repo: tempfile::tempdir().unwrap(),
        };
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(env.repo.path())
            .status()
            .unwrap();
        env
    }

    fn rt(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rt"))
            .args(args)
            .current_dir(self.repo.path())
            .env("RT_HOME", self.home.path())
            .env("RT_ENDPOINT", "http://127.0.0.1:9")
            .env("RT_TOKEN", "rt_test")
            .env_remove("RT_SESSION")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env("RT_AGENT_HARNESS", "test")
            .env("RT_AGENT_SESSION", "agent-1")
            .output()
            .unwrap()
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.repo.path().join(name), text).unwrap();
    }
}

#[test]
fn document_errors_are_reported_before_contacting_the_server() {
    let env = Env::new();
    env.write("bad.md", "## A\n```decision\nkind: go-nogo\n```\n");
    let out = env.rt(&["review", "open", "bad.md"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("line 2: decision block has no `id`"),
        "{stderr}"
    );
}

#[test]
fn agent_instructions_are_printed() {
    let out = Env::new().rt(&["agent-instructions"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("rt review open"));
}

#[test]
fn without_an_instance_rt_says_how_to_get_one() {
    let env = Env::new();
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["whoami"])
        .current_dir(env.repo.path())
        .env("RT_HOME", env.home.path())
        .env_remove("RT_ENDPOINT")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("run `rt init`"), "{err}");
}

#[test]
fn a_token_goes_over_https_only() {
    let env = Env::new();
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["whoami"])
        .current_dir(env.repo.path())
        .env("RT_HOME", env.home.path())
        .env("RT_ENDPOINT", "http://rt.example.com")
        .env("RT_TOKEN", "rt_test")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("plain HTTP"), "{err}");
}
