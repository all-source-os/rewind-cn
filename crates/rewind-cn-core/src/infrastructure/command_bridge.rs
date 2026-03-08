use allframe::cqrs::{Command, CommandError, CommandHandler, CommandResult, ValidationError};
use async_trait::async_trait;

use crate::application::commands;
use crate::application::handlers;
use crate::domain::error::RewindError;
use crate::domain::events::RewindEvent;

// --- Error conversion ---

fn rewind_error_to_command_error(e: RewindError) -> CommandError {
    match e {
        RewindError::Validation { field, message } => {
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
impl CommandHandler<CreateTaskCmd, RewindEvent> for CreateTaskBridge {
    async fn handle(&self, cmd: CreateTaskCmd) -> CommandResult<RewindEvent> {
        handlers::handle_create_task(cmd.0).map_err(rewind_error_to_command_error)
    }
}

pub struct AssignTaskBridge;

#[async_trait]
impl CommandHandler<AssignTaskCmd, RewindEvent> for AssignTaskBridge {
    async fn handle(&self, cmd: AssignTaskCmd) -> CommandResult<RewindEvent> {
        handlers::handle_assign_task(cmd.0).map_err(rewind_error_to_command_error)
    }
}

pub struct StartTaskBridge;

#[async_trait]
impl CommandHandler<StartTaskCmd, RewindEvent> for StartTaskBridge {
    async fn handle(&self, cmd: StartTaskCmd) -> CommandResult<RewindEvent> {
        handlers::handle_start_task(cmd.0).map_err(rewind_error_to_command_error)
    }
}

pub struct CompleteTaskBridge;

#[async_trait]
impl CommandHandler<CompleteTaskCmd, RewindEvent> for CompleteTaskBridge {
    async fn handle(&self, cmd: CompleteTaskCmd) -> CommandResult<RewindEvent> {
        handlers::handle_complete_task(cmd.0).map_err(rewind_error_to_command_error)
    }
}

pub struct FailTaskBridge;

#[async_trait]
impl CommandHandler<FailTaskCmd, RewindEvent> for FailTaskBridge {
    async fn handle(&self, cmd: FailTaskCmd) -> CommandResult<RewindEvent> {
        handlers::handle_fail_task(cmd.0).map_err(rewind_error_to_command_error)
    }
}

pub struct CreateEpicBridge;

#[async_trait]
impl CommandHandler<CreateEpicCmd, RewindEvent> for CreateEpicBridge {
    async fn handle(&self, cmd: CreateEpicCmd) -> CommandResult<RewindEvent> {
        handlers::handle_create_epic(cmd.0).map_err(rewind_error_to_command_error)
    }
}

pub struct CompleteEpicBridge;

#[async_trait]
impl CommandHandler<CompleteEpicCmd, RewindEvent> for CompleteEpicBridge {
    async fn handle(&self, cmd: CompleteEpicCmd) -> CommandResult<RewindEvent> {
        handlers::handle_complete_epic(cmd.0).map_err(rewind_error_to_command_error)
    }
}

pub struct StartSessionBridge;

#[async_trait]
impl CommandHandler<StartSessionCmd, RewindEvent> for StartSessionBridge {
    async fn handle(&self, cmd: StartSessionCmd) -> CommandResult<RewindEvent> {
        handlers::handle_start_session(cmd.0).map_err(rewind_error_to_command_error)
    }
}

pub struct EndSessionBridge;

#[async_trait]
impl CommandHandler<EndSessionCmd, RewindEvent> for EndSessionBridge {
    async fn handle(&self, cmd: EndSessionCmd) -> CommandResult<RewindEvent> {
        handlers::handle_end_session(cmd.0).map_err(rewind_error_to_command_error)
    }
}
