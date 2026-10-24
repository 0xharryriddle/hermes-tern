use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hermes-tern"))
        .args(args)
        .env("TERN_TSP", "0")
        .env_remove("TERN_TSP_RECORD")
        .output()
        .expect("run hermes-tern")
}

#[test]
fn help_succeeds_without_terminal_control_sequences() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("Usage:"));
    assert!(text.contains("preview"));
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn no_arguments_show_help_instead_of_launching_an_agent() {
    let out = run(&[]);
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().contains("Usage:"));
}

#[test]
fn version_reports_this_build() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("hermes-tern {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn preview_is_plain_when_native_surfaces_are_disabled() {
    let out = run(&["preview"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("Preview only"));
    assert!(text.contains("No agent is connected"));
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn unknown_command_is_not_silently_accepted() {
    let out = run(&["not-a-command"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8(out.stderr).unwrap().contains("Usage:"));
}

#[test]
fn extra_arguments_are_rejected_instead_of_ignored() {
    let out = run(&["preview", "--execute"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn unimplemented_run_command_cannot_claim_success() {
    let out = run(&["run"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn doctor_is_an_explicit_command_and_missing_runtime_is_an_error() {
    let out = run(&["doctor", "--help"]);
    assert!(out.status.success());
    let out = run(&[
        "doctor",
        "--hermes-root",
        "/nonexistent/hermes",
        "--python",
        "/nonexistent/python",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

#[test]
fn chat_and_sessions_help_are_available_without_connecting() {
    assert!(run(&["chat", "--help"]).status.success());
    assert!(run(&["sessions", "--help"]).status.success());
}

#[test]
fn history_help_and_arguments_are_checked_without_discovering_a_runtime() {
    let help = run(&["history", "--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("--session"));
    assert!(text.contains("--page"));
    assert_eq!(run(&["history"]).status.code(), Some(2));
    assert_eq!(
        run(&["history", "--session", "s", "--page", "0"])
            .status
            .code(),
        Some(2)
    );
    let invalid = run(&[
        "history",
        "--session",
        "bad\u{1b}",
        "--python",
        "/nonexistent/python",
    ]);
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    assert!(
        String::from_utf8(invalid.stderr)
            .unwrap()
            .contains("session identity")
    );
}

#[test]
fn chat_closed_input_without_query_fails_before_runtime_lookup() {
    let out = run(&["chat", "--plain", "--python", "/nonexistent/python"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().contains("query"));
    assert!(out.stdout.is_empty());
}

#[test]
fn coordinator_help_and_missing_state_are_honest() {
    assert!(run(&["serve", "--help"]).status.success());
    assert!(run(&["coord", "--help"]).status.success());
    let out = run(&["coord", "--state", "/nonexistent/ht-state", "status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}
