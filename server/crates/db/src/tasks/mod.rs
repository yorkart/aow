//! Persistent tasks, role conversation policies, explicit reports and message delivery.
mod conversations;
mod events;
mod inbox;
mod messages;
mod status;
mod store;
#[cfg(test)]
mod tests;
pub use conversations::ConversationPolicy;
pub use events::{EventKind, EventTarget, TaskEvent};
pub use inbox::{InboxEvent, InboxPage};
pub use messages::{DeliveryState, MessageDelivery, MessageSource};
pub use status::TaskStatus;
pub use store::{TaskDb, TaskRecord, TaskResource};
