//! Retrieval subsystem foundation (Sprint 2A).
//!
//! Task 2.2 introduces the lazy bundled-model runtime in [`model`]; Task 2.3
//! adds deterministic source chunking in [`chunking`]; Task 2.4 owns the
//! durable FTS/semantic index worker in [`worker`]; Task 2.5 owns the
//! immutable query index, journal publication, and activation in [`index`];
//! Sprint 3 Task 3.1 adds the concrete persisted-scope retrieval service in
//! [`service`]; Task 3.2 adds rank fusion, cross-channel evidence
//! deduplication, meeting aggregation, and local reranking in [`ranking`].
//! Semantic components are strictly additive: every failure is a typed
//! availability result so callers fall back to FTS5 while any component is
//! unavailable.

pub mod agent;
pub mod chunking;
pub mod commands;
pub mod contracts;
pub mod hydration;
pub mod index;
pub mod model;
pub mod ranking;
pub mod service;
pub mod worker;

#[cfg(test)]
mod tests;

pub use agent::{
    parse_planner_action, run_deep_preparation, DeepBounds, DeepPreparation, DeepPreparationError,
    DeepProgressCallback, DeepProgressEvent, DeepProgressStage, PlannerAction, PlannerActionError,
    PlannerFailure, PlannerGeneration, PlannerStatus, SharedClientPlanner, DEEP_PREPARATION_BUDGET,
    PLANNER_CALL_TIMEOUT, PLANNER_MAX_EXPANDS_PER_ROUND, PLANNER_MAX_INPUT_CHARS,
    PLANNER_MAX_OPENS_PER_ROUND, PLANNER_MAX_OPENS_TOTAL, PLANNER_MAX_OUTPUT_BYTES,
    PLANNER_MAX_OUTPUT_TOKENS, PLANNER_MAX_QUERIES_PER_ROUND, PLANNER_MAX_QUERY_CHARS,
    PLANNER_MAX_ROUNDS, PLANNER_SCHEMA_VERSION,
};
pub use chunking::{chunk_meeting, ChunkerConfig, SemanticDocument, TokenizerPolicy};
pub use contracts::{
    validate_hybrid_query, HybridContextResponse, HybridMeetingResult, HybridProvenance,
    HybridRetrievalStatus, HybridScope, HybridSearchResponse, HybridSource,
    MAX_HYBRID_CONTEXT_CHARS, MAX_HYBRID_QUERY_CHARS, MAX_HYBRID_SEARCH_MEETINGS,
    MAX_HYBRID_SEARCH_RESULTS, SEARCH_HYDRATION_BACKFILL,
};
pub use hydration::{
    hydrate_broad_scope_context, hydrate_context, hydrate_context_with_broad_coverage,
    hydrate_search_context, HydratedContext, HydratedMeeting, HydratedSource,
};
pub use index::{QueryIndexService, ScopeFilter, SearchFailure, VectorHit};
pub use model::{get_or_load, RetrievalModelError, RetrievalModels};
pub use ranking::{
    aggregate_meetings, apply_rerank, concept_coverage, coverage_regions, dedupe_candidates, fuse,
    select_rerank_head, title_overlap, AggregationTerms, FusedEvidence, RankedEvidence,
    RankedMeeting, RankingConfig, RankingOutcome, RerankFallback, SegmentOrder, TitleMatch,
    CHAT_RERANK_DEPTH, CONCEPT_DELTA, RERANK_GAMMA, RRF_K, SEARCH_RERANK_DEPTH, SUPPORT_ALPHA,
    SUPPORT_CAP, SUPPORT_WINDOW, TITLE_BETA, W_LEXICAL, W_VECTOR,
};
pub use service::{
    CoreTermLanguage, EvidenceProvenance, LexicalMode, PersistedRetrievalScope, QueryVariantKind,
    RankedRetrieval, ResolvedScope, RetrievalChannel, RetrievalError, RetrievalLimits,
    RetrievalPurpose, RetrievalRequest, RetrievalResult, RetrievalService, RetrievedEvidence,
    SemanticFallbackReason, SourceAlias, HYBRID_CANDIDATES_PER_VARIANT, MAX_ALLOWED_MEETING_IDS,
};
pub use worker::RetrievalLifecycle;
