//! Bibliothèque interne de `quarity-back`.
//!
//! Expose les modules du back en crate-lib pour qu'ils soient réutilisables par le
//! binaire (`main.rs`) ET par les tests d'intégration (`tests/`). Aucun changement de
//! comportement : `main.rs` consomme ce crate au lieu de redéclarer les modules.

pub mod ch;
pub mod config;
pub mod db;
pub mod error;
pub mod redis_store;
pub mod routes;
pub mod security;
pub mod state;
