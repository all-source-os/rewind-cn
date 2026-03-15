use std::path::Path;

const REPO: &str = "all-source-os/rewind-cn";

pub async fn execute(message: String, attach_report: bool) -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    // Optionally generate a report
    let report_json = if attach_report && Path::new(".rewind").exists() {
        super::report::execute(None, false).await.ok();
        // Find the most recent report file
        std::fs::read_dir(".")
            .ok()
            .and_then(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with("rewind-report-")
                    })
                    .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
            })
            .and_then(|entry| std::fs::read_to_string(entry.path()).ok())
            .map(|s| {
                // Truncate to 64KB for GitHub issue body limits
                if s.len() > 65536 {
                    format!("{}...\n(truncated)", &s[..65536])
                } else {
                    s
                }
            })
    } else {
        None
    };

    let body = format_issue_body(&message, version, os, arch, report_json.as_deref());

    // Try gh CLI first
    if gh_is_available() {
        match create_github_issue(&message, &body) {
            Ok(url) => {
                eprintln!("Feedback submitted: {url}");
                return Ok(());
            }
            Err(e) => {
                eprintln!("gh CLI failed ({e}), falling back to manual template");
            }
        }
    }

    // Fallback: print template for manual submission
    eprintln!("Could not submit automatically. Please create an issue manually:");
    eprintln!();
    let encoded_title = urlencoding(&message);
    let encoded_body = urlencoding(&body);
    eprintln!(
        "https://github.com/{REPO}/issues/new?title={encoded_title}&body={encoded_body}&labels=user-feedback"
    );
    eprintln!();
    eprintln!("Or copy the following into a new issue at https://github.com/{REPO}/issues/new");
    eprintln!("---");
    println!("{body}");

    Ok(())
}

fn format_issue_body(
    message: &str,
    version: &str,
    os: &str,
    arch: &str,
    report: Option<&str>,
) -> String {
    let mut body = format!(
        "## Feedback\n\n{message}\n\n## Environment\n\n- rewind: v{version}\n- os: {os}/{arch}\n"
    );

    if let Some(report) = report {
        body.push_str(
            "\n## Diagnostic Report\n\n<details>\n<summary>Click to expand</summary>\n\n```json\n",
        );
        body.push_str(report);
        body.push_str("\n```\n\n</details>\n");
    }

    body
}

fn gh_is_available() -> bool {
    std::process::Command::new("gh")
        .args(["auth", "status"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn create_github_issue(title: &str, body: &str) -> Result<String, String> {
    let output = std::process::Command::new("gh")
        .args([
            "issue",
            "create",
            "--repo",
            REPO,
            "--title",
            &format!("[feedback] {title}"),
            "--body",
            body,
            "--label",
            "user-feedback",
        ])
        .output()
        .map_err(|e| format!("Failed to run gh: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Minimal percent-encoding for URL query parameters.
fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => result.push_str("%20"),
            '\n' => result.push_str("%0A"),
            '#' => result.push_str("%23"),
            '&' => result.push_str("%26"),
            '=' => result.push_str("%3D"),
            '?' => result.push_str("%3F"),
            '+' => result.push_str("%2B"),
            '%' => result.push_str("%25"),
            _ if c.is_ascii_alphanumeric() || "-_.~:/".contains(c) => result.push(c),
            _ => {
                for b in c.to_string().as_bytes() {
                    result.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_body_formatting() {
        let body = format_issue_body("Things are broken", "0.1.0", "macos", "aarch64", None);
        assert!(body.contains("Things are broken"));
        assert!(body.contains("v0.1.0"));
        assert!(body.contains("macos/aarch64"));
    }

    #[test]
    fn issue_body_with_report() {
        let body = format_issue_body(
            "Help",
            "0.1.0",
            "linux",
            "x86_64",
            Some(r#"{"version":"0.1.0"}"#),
        );
        assert!(body.contains("<details>"));
        assert!(body.contains(r#"{"version":"0.1.0"}"#));
    }
}
