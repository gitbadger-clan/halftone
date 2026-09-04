// crates/halftone-cli/tests/completions.rs
//! Dynamic completion end to end: the shell calls `COMPLETE=<shell> ht -- <words>` and
//! reads candidates from stdout. Uses the bash wire protocol since it is the simplest.

use std::process::Command;

fn ht() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ht"))
}

fn bash_complete(words: &[&str], index: usize) -> Vec<String> {
    let out = ht()
        .env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", index.to_string())
        .arg("--")
        .args(words)
        .output()
        .expect("run ht");
    assert!(
        out.status.success(),
        "completion exited {:?}\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn flags_and_values_complete_through_the_binary() {
    let c = bash_complete(&["ht", "inspect", "--on"], 2);
    assert!(c.iter().any(|x| x == "--only"), "{c:?}");

    let c = bash_complete(&["ht", "inspect", "--only", "container,"], 3);
    assert!(c.iter().any(|x| x == "container,manifest"), "{c:?}");

    let c = bash_complete(&["ht", "fingerprint", "--class", "ph"], 3);
    assert_eq!(c, vec!["phone"]);

    let c = bash_complete(&["ht", "inspect", "--trust-list", "n"], 3);
    assert!(c.iter().any(|x| x == "none"), "{c:?}");
}

#[test]
fn registration_snippet_available_both_ways() {
    for shell in ["bash", "zsh", "fish"] {
        let sub = ht().args(["completions", shell]).output().unwrap();
        assert!(sub.status.success(), "ht completions {shell}");
        let sub = String::from_utf8(sub.stdout).unwrap();
        assert!(sub.contains("COMPLETE"), "{shell}: {sub}");

        let env = ht().env("COMPLETE", shell).output().unwrap();
        assert!(env.status.success(), "COMPLETE={shell} ht");
        let env = String::from_utf8(env.stdout).unwrap();
        assert!(env.contains("COMPLETE"), "{shell}: {env}");
    }
}

#[test]
fn empty_complete_var_is_a_normal_run() {
    let out = ht().env("COMPLETE", "").arg("--version").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("ht"));
}
