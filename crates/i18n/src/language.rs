use std::{cell::Cell, future::Future};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en-US")]
    English,
    #[serde(rename = "zh-CN")]
    Chinese,
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(Self::parse(&value))
    }
}

impl Language {
    pub fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().replace('_', "-").as_str() {
            "zh" | "zh-cn" | "zh-hans" | "zh-hans-cn" => Self::Chinese,
            _ => Self::English,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::English => "en-US",
            Self::Chinese => "zh-CN",
        }
    }
}

tokio::task_local! { static LANGUAGE: Language; }
thread_local! { static BLOCKING_LANGUAGE: Cell<Language> = const { Cell::new(Language::English) }; }

pub fn current_language() -> Language {
    LANGUAGE
        .try_with(|language| *language)
        .unwrap_or_else(|_| BLOCKING_LANGUAGE.get())
}

pub async fn scope<T>(language: Language, future: impl Future<Output = T>) -> T {
    LANGUAGE.scope(language, future).await
}

/// Propagate language explicitly because Tokio does not inherit task-local values.
pub fn spawn_blocking<F, R>(operation: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let language = current_language();
    tokio::task::spawn_blocking(move || {
        struct Restore(Language);
        impl Drop for Restore {
            fn drop(&mut self) {
                BLOCKING_LANGUAGE.set(self.0);
            }
        }
        let _restore = Restore(BLOCKING_LANGUAGE.replace(language));
        operation()
    })
}

pub fn with_language<T>(language: Language, operation: impl FnOnce() -> T) -> T {
    LANGUAGE.sync_scope(language, operation)
}

pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let language = current_language();
    tokio::spawn(scope(language, future))
}
