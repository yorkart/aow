//! Locale context, negotiation and interpolation without product catalogs.
mod catalog;
pub mod http;
mod language;
pub use catalog::Catalog;
pub use language::*;
#[cfg(test)]
mod tests;
