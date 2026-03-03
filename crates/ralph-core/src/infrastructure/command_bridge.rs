use allframe::cqrs::{Command, CommandError, CommandHandler, CommandResult, ValidationError};
use async_trait::async_trait;

use crate::application::commands;
use crate::application::handlers;
use crate::domain::error::RalphError;
use crate::domain::events::RalphEvent;

// --- Error conversion ---

fn ralph_error_to_command_error(e: RalphError) -> CommandError {
    match e {
        RalphError::Validation { field, message } => {
            CommandError::Validation(vec![ValidationError::new(field, message)])
        }
        other => CommandError::Internal(other.to_string()),
    }
}

// --- Newtype wrappers ---

pub struct CreateTaskCmd(pub commands::CreateTask);
impl Command for CreateTaskCmd {}

pub struct AssignTaskCmd(pub commands::AssignTask);
impl Command for AssignTaskCmd {}

pub struct StartTaskCmd(pub commands::StartTask);
impl Command for StartTaskCmd {}

pub struct CompleteTaskCmd(pub commands::CompleteTask);
impl Command for CompleteTaskCmd {}

pub struct FailTaskCmd(pub commands::FailTask);
impl Command for FailTaskCmd {}

pub struct CreateEpicCmd(pub commands::CreateEpic);
impl Command for CreateEpicCmd {}

pub struct CompleteEpicCmd(pub commands::CompleteEpic);
impl Command for CompleteEpicCmd {}

pub struct StartSessionCmd(pub commands::StartSession);
impl Command for StartSessionCmd {}

pub struct EndSessionCmd(pub commands::EndSession);
impl Command for EndSessionCmd {}

// --- Handlers ---

pub struct CreateTaskBridge;

#[async_trait]
impl CommandHandler<CreateTaskCmd, RalphEvent> for CreateTaskBridge {
    async fn handle(&self, cmd: CreateTaskCmd) -> CommandResult<RalphEvent> {
        handlers::handle_create_task(cmd.0).map_err(ralph_error_to_command_error)
    }
}

pub struct AssignTaskBridge;

#[async_trait]
impl CommandHandler<AssignTaskCmd, RalphEvent> for AssignTaskBridge {
    async fn handle(&self, cmd: AssignTaskCmd) -> CommandResult<RalphEvent> {
        handlers::handle_assign_task(cmd.0).map_err(ralph_error_to_command_error)
    }
}

pub struct StartTaskBridge;

#[async_trait]
impl CommandHandler<StartTaskCmd, RalphEvent> for StartTaskBridge {
    async fn handle(&self, cmd: StartTaskCmd) -> CommandResult<RalphEvent> {
        handlers::handle_start_task(cmd.0).map_err(ralph_error_to_command_error)
    }
}

pub struct CompleteTaskBridge;

#[async_trait]
impl CommandHandler<CompleteTaskCmd, RalphEvent> for CompleteTaskBridge {
    async fn handle(&self, cmd: CompleteTaskCmd) -> CommandResult<RalphEvent> {
        handlers::handle_complete_task(cmd.0).map_err(ralph_error_to_command_error)
    }
}

pub struct FailTaskBridge;

#[async_trait]
impl CommandHandler<FailTaskCmd, RalphEvent> for FailTaskBridge {
    async fn handle(&self, cmd: FailTaskCmd) -> CommandResult<RalphEvent> {
        handlers::handle_fail_task(cmd.0).map_err(ralph_error_to_command_error)
    }
}

pub struct CreateEpicBridge;

#[async_trait]
impl CommandHandler<CreateEpicCmd, RalphEvent> for CreateEpicBridge {
    async fn handle(&self, cmd: CreateEpicCmd) -> CommandResult<RalphEvent> {
        handlers::handle_create_epic(cmd.0).map_err(ralph_error_to_command_error)
    }
}

pub struct CompleteEpicBridge;

#[async_trait]
impl CommandHandler<CompleteEpicCmd, RalphEvent> for CompleteEpicBridge {
    async fn handle(&self, cmd: CompleteEpicCmd) -> CommandResult<RalphEvent> {
        handlers::handle_complete_epic(cmd.0).map_err(ralph_error_to_command_error)
    }
}

pub struct StartSessionBridge;

#[async_trait]
impl CommandHandler<StartSessionCmd, RalphEvent> for StartSessionBridge {
    async fn handle(&self, cmd: StartSessionCmd) -> CommandResult<RalphEvent> {
        handlers::handle_start_session(cmd.0).map_err(ralph_error_to_command_error)
    }
}

pub struct EndSessionBridge;

#[async_trait]
impl CommandHandler<EndSessionCmd, RalphEvent> for EndSessionBridge {
    async fn handle(&self, cmd: EndSessionCmd) -> CommandResult<RalphEvent> {
        handlers::handle_end_session(cmd.0).map_err(ralph_error_to_command_error)
    }
}
