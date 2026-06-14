//! Adapters from dependency-light domain tasks into the desktop Iced runtime.

use ferrex_player_foundation::domain::DomainTask;
use iced::Task;

/// Convert an extracted-domain task into an `iced::Task`.
pub fn into_iced_task<Message>(task: DomainTask<Message>) -> Task<Message>
where
    Message: Send + 'static,
{
    match task {
        DomainTask::None => Task::none(),
        DomainTask::Done(messages) => {
            Task::batch(messages.into_iter().map(Task::done))
        }
        DomainTask::Future(future) => Task::perform(future, |message| message),
        DomainTask::Batch(tasks) => {
            Task::batch(tasks.into_iter().map(into_iced_task))
        }
    }
}
