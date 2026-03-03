use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Plan {
    pub epic_title: String,
    pub epic_description: String,
    pub tasks: Vec<PlannedTask>,
}

#[derive(Debug, Serialize)]
pub struct PlannedTask {
    pub title: String,
    pub description: String,
}

/// Phase 1: wraps the input as a single epic + single task.
pub fn passthrough_plan(input: &str) -> Plan {
    let first_line = input.lines().next().unwrap_or(input);
    let title = if first_line.len() > 80 {
        format!("{}...", &first_line[..77])
    } else {
        first_line.to_string()
    };

    Plan {
        epic_title: title.clone(),
        epic_description: input.to_string(),
        tasks: vec![PlannedTask {
            title,
            description: input.to_string(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_single_line() {
        let plan = passthrough_plan("Build auth");
        assert_eq!(plan.epic_title, "Build auth");
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].title, "Build auth");
    }

    #[test]
    fn passthrough_multi_line() {
        let plan = passthrough_plan("Build auth\nNeeds OAuth + JWT");
        assert_eq!(plan.epic_title, "Build auth");
        assert_eq!(plan.tasks[0].description, "Build auth\nNeeds OAuth + JWT");
    }

    #[test]
    fn passthrough_truncates_long_title() {
        let long = "x".repeat(100);
        let plan = passthrough_plan(&long);
        assert!(plan.epic_title.len() <= 80);
        assert!(plan.epic_title.ends_with("..."));
    }
}
