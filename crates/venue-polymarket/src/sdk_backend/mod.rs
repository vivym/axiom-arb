//! Experimental SDK-backed backend for the future capability-oriented gateway.
//!
//! Phase A keeps this surface internal while the legacy transport-oriented exports
//! remain available for existing callers.

pub mod clob;
pub mod ws;

pub use clob::{LiveClobSdkApi, PolymarketClobApi};
pub use ws::{LiveWsSdkApi, PolymarketStreamApi};
