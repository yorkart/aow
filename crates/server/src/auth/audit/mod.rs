//! Authentication evidence and event auditing.

mod event;
mod evidence;
#[cfg(test)]
mod tests;

#[cfg(test)]
use evidence::{safe_path, safe_url};

pub(super) use event::Event;
pub(super) use evidence::{Evidence, clean};
