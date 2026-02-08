//! Integration tests for the crew CLI.

use std::process::Command;

/// Get the path to the crew binary.
fn crew_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps
    path.push("crew");
    path
}

#[test]
fn test_help_command() {
    let output = Command::new(crew_binary())
        .arg("--help")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("crew-rs"));
    assert!(stdout.contains("init"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("resume"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("clean"));
    assert!(stdout.contains("completions"));
}

#[test]
fn test_version_command() {
    let output = Command::new(crew_binary())
        .arg("--version")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("crew"));
}

#[test]
fn test_init_help() {
    let output = Command::new(crew_binary())
        .args(["init", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initialize"));
    assert!(stdout.contains("--defaults"));
}

#[test]
fn test_run_help() {
    let output = Command::new(crew_binary())
        .args(["run", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--provider"));
    assert!(stdout.contains("--model"));
    assert!(stdout.contains("--max-iterations"));
    assert!(stdout.contains("--max-tokens"));
    assert!(stdout.contains("--verbose"));
    assert!(stdout.contains("--no-retry"));
    assert!(stdout.contains("--coordinate"));
}

#[test]
fn test_resume_help() {
    let output = Command::new(crew_binary())
        .args(["resume", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Resume"));
    assert!(stdout.contains("--max-iterations"));
    assert!(stdout.contains("--max-tokens"));
}

#[test]
fn test_clean_help() {
    let output = Command::new(crew_binary())
        .args(["clean", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Clean"));
    assert!(stdout.contains("--all"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_completions_help() {
    let output = Command::new(crew_binary())
        .args(["completions", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("completions"));
}

#[test]
fn test_completions_bash() {
    let output = Command::new(crew_binary())
        .args(["completions", "bash"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Bash completions should contain function definitions
    assert!(stdout.contains("_crew"));
}

#[test]
fn test_completions_zsh() {
    let output = Command::new(crew_binary())
        .args(["completions", "zsh"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Zsh completions should contain compdef
    assert!(stdout.contains("#compdef"));
}

#[test]
fn test_completions_fish() {
    let output = Command::new(crew_binary())
        .args(["completions", "fish"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Fish completions should contain complete command
    assert!(stdout.contains("complete"));
}

#[test]
fn test_init_defaults_in_temp_dir() {
    let temp_dir = tempfile::tempdir().unwrap();

    let output = Command::new(crew_binary())
        .args(["init", "--defaults", "--cwd"])
        .arg(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());

    // Check config file was created
    let config_path = temp_dir.path().join(".crew").join("config.json");
    assert!(config_path.exists());

    // Check config content
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("anthropic"));
    assert!(content.contains("claude-sonnet-4-20250514"));
}

#[test]
fn test_clean_no_crew_dir() {
    let temp_dir = tempfile::tempdir().unwrap();

    let output = Command::new(crew_binary())
        .args(["clean", "--cwd"])
        .arg(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No .crew directory"));
}

#[test]
fn test_clean_empty_crew_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp_dir.path().join(".crew")).unwrap();

    let output = Command::new(crew_binary())
        .args(["clean", "--cwd"])
        .arg(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Nothing to clean"));
}

#[test]
fn test_clean_dry_run() {
    let temp_dir = tempfile::tempdir().unwrap();
    let crew_dir = temp_dir.path().join(".crew");
    let tasks_dir = crew_dir.join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(tasks_dir.join("task1.json"), "{}").unwrap();

    let output = Command::new(crew_binary())
        .args(["clean", "--dry-run", "--cwd"])
        .arg(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Would remove"));
    assert!(stdout.contains("task1.json"));
    assert!(stdout.contains("Dry run"));

    // File should still exist
    assert!(tasks_dir.join("task1.json").exists());
}

#[test]
fn test_clean_removes_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let crew_dir = temp_dir.path().join(".crew");
    let tasks_dir = crew_dir.join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(tasks_dir.join("task1.json"), "{}").unwrap();
    std::fs::write(tasks_dir.join("task2.json"), "{}").unwrap();

    let output = Command::new(crew_binary())
        .args(["clean", "--cwd"])
        .arg(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Cleaned"));

    // Files should be deleted
    assert!(!tasks_dir.join("task1.json").exists());
    assert!(!tasks_dir.join("task2.json").exists());
}

#[test]
fn test_list_no_tasks() {
    let temp_dir = tempfile::tempdir().unwrap();

    let output = Command::new(crew_binary())
        .args(["list", "--cwd"])
        .arg(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    // Should succeed even with no tasks
    assert!(output.status.success());
}

#[test]
fn test_run_missing_goal() {
    let output = Command::new(crew_binary())
        .arg("run")
        .output()
        .expect("Failed to execute command");

    // Should fail with missing required argument
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("required"));
}
