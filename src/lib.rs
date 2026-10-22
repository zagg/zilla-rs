//! Zilla-RS — client FTP/FTPS/SFTP « à la FileZilla », tout en web, en Rust.
//!
//! Bibliothèque et binaire : `zilla serve` lance l'interface, `zilla demo`
//! ouvre un serveur de démonstration, `zilla selftest` vérifie le client.

pub mod api;
pub mod app;
pub mod demosrv;
pub mod fs;
pub mod ftp;
pub mod hub;
pub mod listparse;
pub mod local;
pub mod models;
pub mod net;
pub mod selftest;
pub mod settings;
pub mod sftp;
pub mod store;
pub mod transfers;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Réexport commode pour les tests.
pub use app::App;
pub use hub::LogHub;
pub use models::{Proto, Site};
pub use settings::Settings;
