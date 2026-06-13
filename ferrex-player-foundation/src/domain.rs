//! Generic domain update/event helper types.
//!
//! These containers model the shape of a domain update without knowing about a
//! concrete application message enum, event enum, or UI task type.

use std::{fmt, future::Future, pin::Pin, sync::Arc};

/// Dependency-light asynchronous work emitted by extracted player domains.
///
/// UI shells translate this value into their concrete scheduler. Keeping the
/// representation here prevents data crates from depending on a UI runtime just
/// to express delayed or asynchronous domain messages.
pub enum DomainTask<Message> {
    /// No work should be scheduled.
    None,
    /// Messages that should be re-entered into the application immediately.
    Done(Vec<Message>),
    /// A single asynchronous message producer.
    Future(Pin<Box<dyn Future<Output = Message> + Send + 'static>>),
    /// Multiple task-like values to run together.
    Batch(Vec<DomainTask<Message>>),
}

impl<Message> fmt::Debug for DomainTask<Message>
where
    Message: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("DomainTask::None"),
            Self::Done(messages) => {
                f.debug_tuple("DomainTask::Done").field(messages).finish()
            }
            Self::Future(_) => f.write_str("DomainTask::Future(<opaque>)"),
            Self::Batch(tasks) => {
                f.debug_tuple("DomainTask::Batch").field(tasks).finish()
            }
        }
    }
}

impl<Message> DomainTask<Message> {
    /// Create an empty domain task.
    pub fn none() -> Self {
        Self::None
    }

    /// Create a domain task that immediately emits one message.
    pub fn done(message: Message) -> Self {
        Self::Done(vec![message])
    }

    /// Create a domain task that immediately emits multiple messages.
    pub fn done_many(messages: impl IntoIterator<Item = Message>) -> Self {
        Self::Done(messages.into_iter().collect())
    }

    /// Create a domain task that runs multiple tasks together.
    pub fn batch(tasks: impl IntoIterator<Item = DomainTask<Message>>) -> Self {
        let mut flattened = Vec::new();
        let mut done = Vec::new();

        for task in tasks {
            match task {
                DomainTask::None => {}
                DomainTask::Done(messages) => done.extend(messages),
                DomainTask::Batch(tasks) => flattened.extend(tasks),
                other => flattened.push(other),
            }
        }

        if !done.is_empty() {
            flattened.push(DomainTask::Done(done));
        }

        match flattened.len() {
            0 => DomainTask::None,
            1 => flattened.into_iter().next().unwrap_or(DomainTask::None),
            _ => DomainTask::Batch(flattened),
        }
    }
}

impl<Message> DomainTask<Message>
where
    Message: Send + 'static,
{
    /// Create a domain task from a future and output mapper.
    pub fn perform<Output, Fut, Map>(future: Fut, map: Map) -> Self
    where
        Output: Send + 'static,
        Fut: Future<Output = Output> + Send + 'static,
        Map: FnOnce(Output) -> Message + Send + 'static,
    {
        Self::Future(Box::pin(async move { map(future.await) }))
    }

    /// Map messages produced by this task into another message type.
    pub fn map<Next, Map>(self, map: Map) -> DomainTask<Next>
    where
        Next: Send + 'static,
        Map: Fn(Message) -> Next + Send + Sync + 'static,
    {
        self.map_shared(Arc::new(map))
    }

    fn map_shared<Next>(
        self,
        map: Arc<dyn Fn(Message) -> Next + Send + Sync>,
    ) -> DomainTask<Next>
    where
        Next: Send + 'static,
    {
        match self {
            DomainTask::None => DomainTask::None,
            DomainTask::Done(messages) => DomainTask::Done(
                messages.into_iter().map(|message| map(message)).collect(),
            ),
            DomainTask::Future(future) => {
                DomainTask::Future(Box::pin(async move {
                    let message = future.await;
                    map(message)
                }))
            }
            DomainTask::Batch(tasks) => DomainTask::batch(
                tasks
                    .into_iter()
                    .map(|task| task.map_shared(Arc::clone(&map))),
            ),
        }
    }
}

impl<Message> Default for DomainTask<Message> {
    fn default() -> Self {
        Self::none()
    }
}

/// Result of a domain update operation.
///
/// Contains both messages to process and events to broadcast. The generic
/// parameters keep this helper independent from any concrete Ferrex player
/// domain message type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainUpdate<Message, Event> {
    /// Messages to be processed by the domain or by other domains.
    pub messages: Vec<Message>,
    /// Events to broadcast to interested domains.
    pub events: Vec<Event>,
}

impl<Message, Event> DomainUpdate<Message, Event> {
    /// Create an empty update with no messages or events.
    pub fn none() -> Self {
        Self {
            messages: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Create an update with a single message.
    pub fn message(msg: impl Into<Message>) -> Self {
        Self {
            messages: vec![msg.into()],
            events: Vec::new(),
        }
    }

    /// Create an update with a single event.
    pub fn event(event: Event) -> Self {
        Self {
            messages: Vec::new(),
            events: vec![event],
        }
    }

    /// Create an update with messages and events.
    pub fn with(messages: Vec<Message>, events: Vec<Event>) -> Self {
        Self { messages, events }
    }

    /// Add a message to this update.
    pub fn add_message(mut self, msg: impl Into<Message>) -> Self {
        self.messages.push(msg.into());
        self
    }

    /// Add an event to this update.
    pub fn add_event(mut self, event: Event) -> Self {
        self.events.push(event);
        self
    }

    /// Check if this update contains no messages and no events.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.events.is_empty()
    }
}

impl<Message, Event> Default for DomainUpdate<Message, Event> {
    fn default() -> Self {
        Self::none()
    }
}

/// Result of a domain update that includes a task-like value and events.
///
/// The `Task` parameter is intentionally generic so callers can use a UI task,
/// test double, or a future platform-specific scheduler without coupling this
/// crate to a UI/runtime dependency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainUpdateResult<Task, Event> {
    /// Task-like value to execute; it may produce more application messages.
    pub task: Task,
    /// Events to broadcast to other domains immediately.
    pub events: Vec<Event>,
}

impl<Task, Event> DomainUpdateResult<Task, Event> {
    /// Create a result with just a task-like value.
    pub fn task(task: Task) -> Self {
        Self {
            task,
            events: Vec::new(),
        }
    }

    /// Create a result with task-like value and events.
    pub fn with_events(task: Task, events: Vec<Event>) -> Self {
        Self { task, events }
    }

    /// Add an event to this result.
    pub fn add_event(mut self, event: Event) -> Self {
        self.events.push(event);
        self
    }

    /// Check if this result carries at least one event.
    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
    }
}

impl<Task, Event> DomainUpdateResult<Task, Event>
where
    Task: Default,
{
    /// Create an empty result using the task type's default value.
    pub fn none() -> Self {
        Self {
            task: Task::default(),
            events: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DomainUpdate, DomainUpdateResult};

    #[test]
    fn domain_update_builders_collect_messages_and_events() {
        let update = DomainUpdate::<String, &'static str>::message("hello")
            .add_message("world")
            .add_event("changed");

        assert_eq!(update.messages, ["hello".to_string(), "world".into()]);
        assert_eq!(update.events, ["changed"]);
        assert!(!update.is_empty());
    }

    #[test]
    fn domain_update_result_uses_default_empty_task() {
        let result = DomainUpdateResult::<Option<String>, &'static str>::none();
        assert_eq!(result.task, None);
        assert!(result.events.is_empty());
    }
}
