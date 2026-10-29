//! Shared Moss logic. Both the desktop app and `moss-mcp` build on this crate.

pub mod approvals;
pub mod audit;
pub mod compile;
pub mod entries;
pub mod error;
pub mod external;
pub mod fsutil;
pub mod history;
pub mod ignore;
pub mod names;
pub mod outline;
pub mod overleaf;
pub mod pdfview;
pub mod project;
pub mod search;
pub mod settings;
pub mod setup;
pub mod sync;
pub mod synctex;
pub mod templates;
pub mod tex;
pub mod texcheck;
pub mod textdiff;
pub mod workspace;

pub use error::{Error, Result};
