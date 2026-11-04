use rusqlite::{
    ToSql,
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef},
};
use serde::{Deserialize, Serialize, de::value::StrDeserializer};

/// Business progress, independent of an execution's outcome or a task's archive state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Waiting,
    InReview,
    Done,
    Blocked,
    Cancelled,
}

impl TaskStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Waiting => "waiting",
            Self::InReview => "in_review",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_closed(self) -> bool {
        match self {
            Self::Done | Self::Cancelled => true,
            Self::Todo | Self::InProgress | Self::Waiting | Self::InReview | Self::Blocked => false,
        }
    }
}

impl ToSql for TaskStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(
            self.as_str().as_bytes(),
        )))
    }
}

impl FromSql for TaskStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        // JSON and SQLite accept exactly the same names; unknown values never
        // silently become a default status.
        Self::deserialize(StrDeserializer::<serde::de::value::Error>::new(
            value.as_str()?,
        ))
        .map_err(|error| FromSqlError::Other(Box::new(error)))
    }
}
