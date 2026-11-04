//! SQLite connections and transactional migrations shared by independent products.
//! Callers own database paths, domain names, schemas and business queries.

mod connection;
mod migrations;
mod read_only;

pub use connection::open;
pub use migrations::migrate;
pub use read_only::open as open_read_only;
