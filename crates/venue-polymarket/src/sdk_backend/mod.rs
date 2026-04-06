//! Experimental SDK-backed backend for the future capability-oriented gateway.
//!
//! Phase A keeps this surface internal while the legacy transport-oriented exports
//! remain available for existing callers.

pub mod clob;
pub mod metadata;
pub mod relayer;
pub mod ws;

pub use clob::{LiveClobSdkApi, PolymarketClobApi};
pub use metadata::{LiveMetadataSdkApi, PolymarketMetadataApi};
pub use relayer::{LiveRelayerApi, PolymarketRelayerApi};
pub use ws::{LiveWsSdkApi, PolymarketStreamApi};
