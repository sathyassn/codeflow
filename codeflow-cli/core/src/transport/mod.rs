//! Transport layer for CRDT state synchronization.
//!
//! Provides abstractions for pushing and fetching Loro deltas
//! across machines via git refs.

pub mod gitref;
