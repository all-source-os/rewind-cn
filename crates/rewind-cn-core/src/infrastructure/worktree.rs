use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{debug, warn};

use crate::domain::error::RewindError;

/// Manages git worktrees for parallel task isolation.
pub struct WorktreeManager {
    /// Root of the main repository.
    repo_root: PathBuf,
    /// Base directory for worktrees (e.g., .rewind/worktrees/).
    worktree_base: PathBuf,
}

impl WorktreeManager {
    pub fn new(repo_root: PathBuf) -> Self {
        let worktree_base = repo_root.join(".rewind/worktrees");
        Self {
            repo_root,
            worktree_base,
        }
    }

    /// Create a new worktree for a task, returning the worktree path.
    pub fn create(&self, task_id: &str) -> Result<PathBuf, RewindError> {
        let worktree_path = self.worktree_base.join(task_id);
        let branch_name = format!("rewind/{task_id}");

        // Ensure base dir exists
        std::fs::create_dir_all(&self.worktree_base)
            .map_err(|e| RewindError::Config(format!("Failed to create worktree base dir: {e}")))?;

        let output = Command::new("git")
            .args(["worktree", "add", "-b", &branch_name])
            .arg(&worktree_path)
            .current_dir(&self.repo_root)
            .output()
            .map_err(|e| RewindError::Config(format!("Failed to run git worktree add: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RewindError::Config(format!(
                "git worktree add failed: {stderr}"
            )));
        }

        debug!("Created worktree at {}", worktree_path.display());
        Ok(worktree_path)
    }

    /// Merge changes from a worktree branch back to the current branch.
    pub fn merge_back(&self, task_id: &str) -> Result<(), RewindError> {
        let branch_name = format!("rewind/{task_id}");

        // Cherry-pick all commits from the worktree branch that aren't on HEAD
        let output = Command::new("git")
            .args(["log", "--format=%H", &format!("HEAD..{branch_name}")])
            .current_dir(&self.repo_root)
            .output()
            .map_err(|e| RewindError::Config(format!("Failed to list worktree commits: {e}")))?;

        let commits_str = String::from_utf8_lossy(&output.stdout);
        let commits: Vec<&str> = commits_str.trim().lines().rev().collect();

        if commits.is_empty() || (commits.len() == 1 && commits[0].is_empty()) {
            debug!("No commits to merge from {branch_name}");
            return Ok(());
        }

        for commit in &commits {
            let output = Command::new("git")
                .args(["cherry-pick", commit])
                .current_dir(&self.repo_root)
                .output()
                .map_err(|e| RewindError::Config(format!("Failed to cherry-pick {commit}: {e}")))?;

            if !output.status.success() {
                // Abort the cherry-pick
                let _ = Command::new("git")
                    .args(["cherry-pick", "--abort"])
                    .current_dir(&self.repo_root)
                    .output();

                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(RewindError::Config(format!(
                    "Merge conflict cherry-picking {commit}: {stderr}"
                )));
            }
        }

        debug!("Merged {} commit(s) from {branch_name}", commits.len());
        Ok(())
    }

    /// Clean up a worktree and its branch.
    pub fn cleanup(&self, task_id: &str) {
        let worktree_path = self.worktree_base.join(task_id);
        let branch_name = format!("rewind/{task_id}");

        // Remove worktree
        let result = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .current_dir(&self.repo_root)
            .output();

        match result {
            Ok(output) if !output.status.success() => {
                warn!(
                    "Failed to remove worktree: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                // Fallback: remove directory
                let _ = std::fs::remove_dir_all(&worktree_path);
            }
            Err(e) => {
                warn!("Failed to run git worktree remove: {e}");
                let _ = std::fs::remove_dir_all(&worktree_path);
            }
            _ => {}
        }

        // Delete branch
        let _ = Command::new("git")
            .args(["branch", "-D", &branch_name])
            .current_dir(&self.repo_root)
            .output();

        debug!("Cleaned up worktree for {task_id}");
    }

    /// Check if worktrees are supported (i.e., we're in a git repo).
    pub fn is_available(repo_root: &Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(repo_root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn is_available_in_git_repo() {
        // We're running in a git repo
        let cwd = env::current_dir().unwrap();
        assert!(WorktreeManager::is_available(&cwd));
    }

    #[test]
    fn is_not_available_in_tmp() {
        assert!(!WorktreeManager::is_available(Path::new(
            "/tmp/nonexistent-repo-xyz"
        )));
    }
}
