// Shared synthetic evaluation-corpus types used by both `retrieval_evaluation`
// (Task 1.2 baseline harness) and `model_benchmark` (Task 1.3 model selection).
// Definitions must stay identical to those pinned by Task 1.2; corpus content
// itself lives in `fixtures/corpus.rs`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Language {
    Portuguese,
    English,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    All,
    Folder,
    Meeting,
    Snapshot,
    Today,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeetingState {
    Current,
    Deleted,
    Dirty,
    StaleDerived,
}

#[derive(Clone, Debug)]
pub struct Scope {
    pub kind: ScopeKind,
    pub folder_id: Option<String>,
    pub meeting_id: Option<String>,
    pub allowed_meeting_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Evidence {
    pub id: String,
    pub source_kind: String,
    pub indexed_text: String,
    pub authoritative_text: String,
}

#[derive(Clone, Debug)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub folder_id: Option<String>,
    pub meeting_date: String,
    pub state: MeetingState,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug)]
pub struct EvaluationCase {
    pub id: String,
    pub language: Language,
    pub question: String,
    // Consumed by the Task 1.2 baseline harness (schema validation and
    // follow-up query rewriting); unused inside the Task 1.3 benchmark.
    #[allow(dead_code)]
    pub history: Vec<String>,
    pub rewritten_query: Option<String>,
    pub scope: Scope,
    pub meetings: Vec<Meeting>,
    pub expected_meeting_ids: Vec<String>,
    // Order constraints are validated by the Task 1.2 harness.
    #[allow(dead_code)]
    pub order_constraints: Vec<(String, String)>,
    pub required_evidence_ids: Vec<String>,
    pub required_facts: Vec<String>,
    pub forbidden_facts: Vec<String>,
    // Answer-mode category is pinned by the Task 1.2 schema validation.
    #[allow(dead_code)]
    pub answer_mode: String,
    pub categories: Vec<String>,
    pub critical: bool,
}
