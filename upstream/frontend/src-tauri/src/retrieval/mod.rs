//! Retrieval subsystem foundation (Sprint 2A).
//!
//! Task 2.2 introduces the lazy bundled-model runtime in [`model`]; Task 2.3
//! adds deterministic source chunking in [`chunking`]; Task 2.4 owns the
//! durable FTS/semantic index worker in [`worker`]; Task 2.5 owns the
//! immutable query index, journal publication, and activation in [`index`];
//! Sprint 3 Task 3.1 adds the concrete persisted-scope retrieval service in
//! [`service`]. Semantic components are strictly additive: every failure is a
//! typed availability result so callers fall back to FTS5 while any component
//! is unavailable.

pub mod chunking;
pub mod commands;
pub mod index;
pub mod model;
pub mod service;
pub mod worker;

#[cfg(test)]
mod tests;

pub use chunking::{chunk_meeting, ChunkerConfig, SemanticDocument, TokenizerPolicy};
pub use index::{QueryIndexService, ScopeFilter, SearchFailure, VectorHit};
pub use model::{get_or_load, RetrievalModelError, RetrievalModels};
pub use service::{
    CoreTermLanguage, EvidenceProvenance, LexicalMode, PersistedRetrievalScope, QueryVariantKind,
    ResolvedScope, RetrievalChannel, RetrievalError, RetrievalLimits, RetrievalPurpose,
    RetrievalRequest, RetrievalResult, RetrievalService, RetrievedEvidence, SemanticFallbackReason,
};
pub use worker::RetrievalLifecycle;
