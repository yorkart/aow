mod arguments;
mod output;
mod process;

pub use process::{SearchError, search_files};

#[cfg(test)]
mod tests;
