use std::{fmt, str::FromStr};

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};

use crate::{Run, store::valid_component};

/// Pagination metadata, independent of the resource ID and its generator.
#[derive(Clone, Debug)]
pub struct RunCursor {
    pub(super) started_at: DateTime<Utc>,
    pub(super) id: String,
}

impl From<&Run> for RunCursor {
    fn from(run: &Run) -> Self {
        Self {
            started_at: run.started_at,
            id: run.id.clone(),
        }
    }
}

impl fmt::Display for RunCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.started_at.to_rfc3339(), self.id)
    }
}

impl FromStr for RunCursor {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        ensure!(value.len() <= 256, "执行分页游标不合法");
        let (time, id) = value.split_once('/').context("执行分页游标不合法")?;
        valid_component(id).context("执行分页游标不合法")?;
        let started_at = time.parse().context("执行分页游标不合法")?;
        Ok(Self {
            started_at,
            id: id.into(),
        })
    }
}
