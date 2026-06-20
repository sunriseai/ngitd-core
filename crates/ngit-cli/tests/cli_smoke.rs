use serde_json::Value;
use std::{fs, process::Command};
use tempfile::TempDir;

fn git_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    run_ok(&dir, "git", &["init"]);
    run_ok(&dir, "git", &["config", "user.email", "test@example.com"]);
    run_ok(&dir, "git", &["config", "user.name", "Test User"]);
    fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    run_ok(&dir, "git", &["add", "README.md"]);
    run_ok(&dir, "git", &["commit", "-m", "initial"]);
    dir
}

fn ngit(dir: &TempDir, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ngit"))
        .args(args)
        .current_dir(dir.path())
        .output()
        .unwrap()
}

fn run_ok(dir: &TempDir, cmd: &str, args: &[&str]) {
    let output = Command::new(cmd)
        .args(args)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} failed: {}",
        cmd,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn ngit_json(dir: &TempDir, args: &[&str]) -> Value {
    let output = ngit(dir, args);
    assert!(
        output.status.success(),
        "ngit {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn cli_init_capture_annotation_evidence_accept_history_doctor() {
    let repo = git_repo();
    ngit_json(&repo, &["init", "--json"]);
    fs::write(repo.path().join("README.md"), "hello blackbox\n").unwrap();

    let draft = ngit_json(
        &repo,
        &[
            "capture",
            "--intent",
            "Explain README blackbox rationale.",
            "--json",
        ],
    );
    let draft_id = draft["draft_id"].as_str().unwrap().to_string();
    assert_eq!(draft["annotation_refs"].as_array().unwrap().len(), 1);

    let annotations = ngit_json(&repo, &["annotation", "list", &draft_id, "--json"]);
    assert_eq!(annotations.as_array().unwrap().len(), 1);

    let evidence = ngit_json(
        &repo,
        &[
            "evidence",
            "run",
            &draft_id,
            "--json",
            "--",
            "rustc",
            "--version",
        ],
    );
    assert_eq!(evidence["status"], "passed");

    let accepted = ngit_json(&repo, &["accept", &draft_id, "--json"]);
    let change_id = accepted["change_id"].as_str().unwrap().to_string();
    assert_eq!(accepted["annotation_refs"].as_array().unwrap().len(), 1);

    let history = ngit_json(&repo, &["history", "--json"]);
    assert_eq!(history.as_array().unwrap().len(), 1);

    let lineage = ngit_json(&repo, &["lineage", &change_id, "--json"]);
    assert_eq!(lineage["event_type"], "change_accepted");

    let doctor = ngit_json(&repo, &["doctor", "--json"]);
    assert_eq!(doctor["issues"].as_array().unwrap().len(), 0);
}

#[test]
fn cli_evidence_run_human_output_is_compact() {
    let repo = git_repo();
    ngit_json(&repo, &["init", "--json"]);
    fs::write(repo.path().join("README.md"), "human evidence\n").unwrap();
    let draft = ngit_json(&repo, &["capture", "--json"]);
    let draft_id = draft["draft_id"].as_str().unwrap().to_string();

    let output = ngit(
        &repo,
        &["evidence", "run", &draft_id, "--", "rustc", "--version"],
    );
    assert!(output.status.success());
    let body = String::from_utf8_lossy(&output.stdout);
    assert!(body.starts_with("evidence evidence-"));
    assert!(body.contains(" passed"));
}

#[test]
fn cli_watch_capture_persists_event_and_respects_policy() {
    let repo = git_repo();
    ngit_json(&repo, &["init", "--json"]);
    fs::write(repo.path().join("README.md"), "changed for watch\n").unwrap();

    let no_capture = ngit_json(&repo, &["watch", "--capture", "--once", "--json"]);
    assert!(no_capture["draft"].is_null());
    assert_eq!(no_capture["event"]["event_type"], "repo_changed");

    fs::write(
        repo.path().join(".ngit/policies/capture.json"),
        r#"{
  "schema_version": 1,
  "mode": "auto",
  "triggers": ["on_stage"],
  "allow_empty_capture": false,
  "dedupe": {
    "enabled": true,
    "fields": ["branch", "head", "head_parent_count", "changed_files", "staged_digest", "worktree_digest"]
  }
}
"#,
    )
    .unwrap();
    run_ok(&repo, "git", &["add", "README.md"]);
    let captured = ngit_json(&repo, &["watch", "--capture", "--once", "--json"]);
    assert!(captured["draft"]["draft_id"].as_str().is_some());
    assert!(repo.path().join(".ngit/events").read_dir().unwrap().count() >= 2);
}

#[test]
fn cli_schema_export_and_migration_import_legacy_draft() {
    let repo = git_repo();
    ngit_json(&repo, &["init", "--json"]);
    fs::write(repo.path().join("README.md"), "legacy migration\n").unwrap();
    let draft = ngit_json(&repo, &["capture", "--json"]);
    let draft_id = draft["draft_id"].as_str().unwrap().to_string();
    let old_dir = repo.path().join(".ngit/mutations/drafts");
    fs::create_dir_all(&old_dir).unwrap();
    let old_path = old_dir.join(format!("{draft_id}.json"));
    let new_path = repo
        .path()
        .join(".ngit/changes/drafts")
        .join(format!("{draft_id}.json"));
    fs::rename(&new_path, &old_path).unwrap();

    let schema_dir = repo.path().join("schemas");
    let schema = ngit_json(
        &repo,
        &[
            "schema",
            "export",
            "--dir",
            schema_dir.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(schema["files"].as_array().unwrap().len() >= 10);
    assert!(schema_dir.join("draft.schema.json").exists());

    let migration = ngit_json(&repo, &["migrate", "--json"]);
    assert_eq!(migration["imported"]["drafts"], 1);
    assert!(new_path.exists());
}

#[test]
fn cli_tui_read_only_renders_overview() {
    let repo = git_repo();
    ngit_json(&repo, &["init", "--json"]);
    let output = ngit(&repo, &["tui", "--read-only"]);
    assert!(output.status.success());
    let body = String::from_utf8_lossy(&output.stdout);
    assert!(body.contains("ngit tui (read-only)"));
    assert!(body.contains("open drafts"));
}
