//! Integration tests for git worktree manager.
//! These tests create real git repos in temp directories.

use std::process::Command;

use rewind_cn_core::infrastructure::worktree::WorktreeManager;

/// Create a fresh git repo in a temp directory for testing.
fn create_test_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();

    // git init
    let output = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "git init failed");

    // Configure git user for commits (disable GPG signing for test isolation)
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create initial commit (required for worktrees)
    std::fs::write(dir.path().join("README.md"), "# Test repo\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    dir
}

#[test]
fn worktree_create_and_cleanup_roundtrip() {
    let repo = create_test_repo();
    let mgr = WorktreeManager::new(repo.path().to_path_buf());

    // Create worktree
    let wt_path = mgr.create("task-001").unwrap();
    assert!(wt_path.exists(), "Worktree directory should exist");
    assert!(
        wt_path.join("README.md").exists(),
        "Worktree should contain repo files"
    );

    // Verify git worktree list shows it
    let output = Command::new("git")
        .args(["worktree", "list"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let list = String::from_utf8_lossy(&output.stdout);
    assert!(
        list.contains("task-001"),
        "Worktree should appear in git worktree list"
    );

    // Cleanup
    mgr.cleanup("task-001");
    assert!(
        !wt_path.exists(),
        "Worktree directory should be removed after cleanup"
    );

    // Verify branch deleted
    let output = Command::new("git")
        .args(["branch"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        !branches.contains("rewind/task-001"),
        "Branch should be deleted after cleanup"
    );
}

#[test]
fn worktree_merge_back_applies_changes() {
    let repo = create_test_repo();
    let mgr = WorktreeManager::new(repo.path().to_path_buf());

    let wt_path = mgr.create("task-002").unwrap();

    // Make changes in the worktree
    std::fs::write(wt_path.join("new_file.rs"), "fn main() {}").unwrap();
    Command::new("git")
        .args(["add", "new_file.rs"])
        .current_dir(&wt_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Add new file"])
        .current_dir(&wt_path)
        .output()
        .unwrap();

    // Merge back
    mgr.merge_back("task-002").unwrap();

    // Verify file exists in main repo
    assert!(
        repo.path().join("new_file.rs").exists(),
        "Merged file should exist in main repo"
    );

    // Cleanup
    mgr.cleanup("task-002");
}

#[test]
fn worktree_merge_back_no_commits_is_ok() {
    let repo = create_test_repo();
    let mgr = WorktreeManager::new(repo.path().to_path_buf());

    let _wt_path = mgr.create("task-003").unwrap();

    // No commits made in worktree — merge_back should succeed silently
    mgr.merge_back("task-003").unwrap();

    mgr.cleanup("task-003");
}

#[test]
fn worktree_merge_conflict_returns_error() {
    let repo = create_test_repo();
    let mgr = WorktreeManager::new(repo.path().to_path_buf());

    let wt_path = mgr.create("task-004").unwrap();

    // Make a conflicting change in main repo
    std::fs::write(repo.path().join("README.md"), "# Main change\n").unwrap();
    Command::new("git")
        .args(["add", "README.md"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Main change"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Make a conflicting change in worktree
    std::fs::write(wt_path.join("README.md"), "# Worktree change\n").unwrap();
    Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&wt_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Worktree change"])
        .current_dir(&wt_path)
        .output()
        .unwrap();

    // Merge back should fail with conflict
    let result = mgr.merge_back("task-004");
    assert!(result.is_err(), "Merge with conflict should return error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("conflict") || err.contains("cherry-pick"),
        "Error should mention conflict: {err}"
    );

    // Verify main repo is not in a broken cherry-pick state
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let status = String::from_utf8_lossy(&output.stdout);
    // .rewind/ directory is untracked (worktree base), that's fine
    // We just need to verify there's no "U" (unmerged) entries
    assert!(
        !status.contains("UU ") && !status.contains("AA "),
        "Repo should not have unmerged entries after aborted cherry-pick, got: {status}"
    );

    mgr.cleanup("task-004");
}

#[test]
fn worktree_multiple_concurrent_worktrees() {
    let repo = create_test_repo();
    let mgr = WorktreeManager::new(repo.path().to_path_buf());

    let wt1 = mgr.create("task-a").unwrap();
    let wt2 = mgr.create("task-b").unwrap();

    assert!(wt1.exists());
    assert!(wt2.exists());
    assert_ne!(wt1, wt2);

    // Both should have repo content
    assert!(wt1.join("README.md").exists());
    assert!(wt2.join("README.md").exists());

    // Changes in one don't affect the other
    std::fs::write(wt1.join("file_a.rs"), "// task A").unwrap();
    assert!(
        !wt2.join("file_a.rs").exists(),
        "Changes in wt1 should not appear in wt2"
    );

    mgr.cleanup("task-a");
    mgr.cleanup("task-b");
}

#[test]
fn worktree_cleanup_is_idempotent() {
    let repo = create_test_repo();
    let mgr = WorktreeManager::new(repo.path().to_path_buf());

    let _wt = mgr.create("task-idem").unwrap();

    // Double cleanup should not panic
    mgr.cleanup("task-idem");
    mgr.cleanup("task-idem"); // second call — no-op

    // Cleanup of nonexistent worktree should not panic
    mgr.cleanup("nonexistent-task");
}

#[test]
fn worktree_is_available_detects_correctly() {
    let repo = create_test_repo();
    assert!(WorktreeManager::is_available(repo.path()));

    let non_repo = tempfile::tempdir().unwrap();
    assert!(!WorktreeManager::is_available(non_repo.path()));
}
