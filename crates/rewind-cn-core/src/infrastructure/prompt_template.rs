use std::collections::HashMap;
use std::path::Path;

use tera::{Context, Tera};

use crate::domain::error::RewindError;

const DEFAULT_TEMPLATE: &str = include_str!("default_prompt.tera");

/// Render a prompt from a Tera template file, falling back to the embedded default.
///
/// `context_map` entries are injected as top-level template variables.
pub fn render_prompt(
    template_path: &Path,
    context_map: &HashMap<String, String>,
) -> Result<String, RewindError> {
    let template_str = if template_path.exists() {
        std::fs::read_to_string(template_path).map_err(|e| {
            RewindError::Config(format!(
                "Failed to read template {}: {e}",
                template_path.display()
            ))
        })?
    } else {
        DEFAULT_TEMPLATE.to_string()
    };

    let mut tera = Tera::default();
    tera.add_raw_template("prompt", &template_str)
        .map_err(|e| RewindError::Config(format!("Template parse error: {e}")))?;

    let mut ctx = Context::new();
    for (k, v) in context_map {
        ctx.insert(k, v);
    }

    tera.render("prompt", &ctx)
        .map_err(|e| RewindError::Config(format!("Template render error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn render_default_template_full_context() {
        let ctx = HashMap::from([
            ("task".into(), "Implement feature X".into()),
            ("epic".into(), "Epic-42: Platform Overhaul".into()),
            ("progress".into(), "3 of 5 tasks complete".into()),
            ("project_context".into(), "Rust CQRS service".into()),
        ]);
        let result = render_prompt(&PathBuf::from("/nonexistent/template.tera"), &ctx);
        assert!(result.is_ok());
        let rendered = result.unwrap();
        assert!(rendered.contains("Implement feature X"));
        assert!(rendered.contains("Epic-42: Platform Overhaul"));
        assert!(rendered.contains("3 of 5 tasks complete"));
        assert!(rendered.contains("Rust CQRS service"));
    }

    #[test]
    fn render_default_template_partial_context() {
        // Only task provided — optional variables should be gracefully absent
        let ctx = HashMap::from([("task".into(), "Fix the login bug".into())]);
        let result = render_prompt(&PathBuf::from("/nonexistent/template.tera"), &ctx);
        assert!(result.is_ok());
        let rendered = result.unwrap();
        assert!(rendered.contains("Fix the login bug"));
        assert!(!rendered.contains("## Epic"));
        assert!(!rendered.contains("## Progress"));
        assert!(!rendered.contains("## Project Context"));
    }

    #[test]
    fn render_default_template_empty_context() {
        // No variables at all — task should show default value
        let result = render_prompt(&PathBuf::from("/nonexistent/template.tera"), &HashMap::new());
        assert!(result.is_ok());
        let rendered = result.unwrap();
        assert!(rendered.contains("No task specified."));
    }

    #[test]
    fn render_custom_template_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let tpl_path = dir.path().join("custom.tera");
        std::fs::write(&tpl_path, "Task: {{ task_description }}").unwrap();

        let ctx = HashMap::from([("task_description".into(), "Fix bug".into())]);
        let result = render_prompt(&tpl_path, &ctx).unwrap();
        assert_eq!(result, "Task: Fix bug");
    }

    #[test]
    fn render_error_on_invalid_template() {
        let dir = tempfile::tempdir().unwrap();
        let tpl_path = dir.path().join("bad.tera");
        std::fs::write(&tpl_path, "{{ unclosed").unwrap();

        let result = render_prompt(&tpl_path, &HashMap::new());
        assert!(result.is_err());
    }
}
