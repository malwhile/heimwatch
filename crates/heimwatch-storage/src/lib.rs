//! heimwatch-storage: Time-series persistence using sled.
//!
//! This crate provides the StorageLayer interface for persisting and querying
//! application metrics collected by heimwatch collectors.

pub mod db;
mod error;
mod keys;

pub use db::StorageLayer;
pub use error::StorageError;
pub use heimwatch_core::metrics::*;
