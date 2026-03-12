use crate::domain::events::{AcceptanceCriterion, QualityGate, StoryType};
use crate::domain::ids::{AgentId, EpicId, SessionId, TaskId};

pub struct CreateTask {
    pub title: String,
    pub description: String,
    pub epic_id: Option<EpicId>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub story_type: Option<StoryType>,
    pub depends_on: Vec<TaskId>,
}

pub struct AssignTask {
    pub task_id: TaskId,
    pub agent_id: AgentId,
}

pub struct StartTask {
    pub task_id: TaskId,
}

pub struct CompleteTask {
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub discretionary_note: Option<String>,
}

pub struct FailTask {
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub reason: String,
    pub discretionary_note: Option<String>,
}

pub struct CreateEpic {
    pub title: String,
    pub description: String,
    pub quality_gates: Vec<QualityGate>,
}

pub struct CompleteEpic {
    pub epic_id: EpicId,
}

pub struct StartSession;

pub struct EndSession {
    pub session_id: SessionId,
}
