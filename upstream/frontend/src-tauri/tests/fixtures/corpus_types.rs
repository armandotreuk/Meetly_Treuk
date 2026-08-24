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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarrierSourceState {
    Superseded,
    StaleDerived,
    Deleted,
    CurrentAuthoritative,
    CurrentTopicalNeighbour,
}

impl CarrierSourceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Superseded => "superseded/draft",
            Self::StaleDerived => "stale-derived",
            Self::Deleted => "deleted",
            Self::CurrentAuthoritative => "current-authoritative-expected",
            Self::CurrentTopicalNeighbour => "current-topical-neighbour",
        }
    }

    fn is_retrieval_stage(self) -> bool {
        matches!(self, Self::Superseded | Self::StaleDerived | Self::Deleted)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForbiddenFactStage {
    Retrieval,
    Answer,
}

impl ForbiddenFactStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Retrieval => "retrieval-stage",
            Self::Answer => "answer-stage-deferred",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForbiddenCarrier {
    pub meeting_id: String,
    pub evidence_id: String,
    pub state: CarrierSourceState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForbiddenFactClassification {
    pub stage: ForbiddenFactStage,
    pub carriers: Vec<ForbiddenCarrier>,
}

fn superseded_signal(text: &str) -> bool {
    let text = text.to_lowercase();
    [
        "rascunho",
        "supersed",
        "descartad",
        "proposta antiga",
        "texto anterior",
        "formato antigo",
        "contagem antiga",
        "regra antiga",
        "old draft",
        "old proposal",
        "superseded",
        "discarded",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

pub fn classify_forbidden_fact(
    case: &EvaluationCase,
    fact: &str,
) -> Result<ForbiddenFactClassification, String> {
    let fact = fact.to_lowercase();
    let mut carriers = Vec::new();
    for meeting in &case.meetings {
        let expected = case.expected_meeting_ids.contains(&meeting.id);
        for evidence in &meeting.evidence {
            let indexed = evidence.indexed_text.to_lowercase();
            let authoritative = evidence.authoritative_text.to_lowercase();
            if !indexed.contains(&fact) && !authoritative.contains(&fact) {
                continue;
            }
            let state = if meeting.state == MeetingState::Deleted {
                CarrierSourceState::Deleted
            } else if expected && authoritative.contains(&fact) {
                CarrierSourceState::CurrentAuthoritative
            } else if indexed.contains(&fact) && !authoritative.contains(&fact) {
                CarrierSourceState::StaleDerived
            } else if superseded_signal(&format!(
                "{} {}",
                meeting.title, evidence.authoritative_text
            )) {
                CarrierSourceState::Superseded
            } else {
                CarrierSourceState::CurrentTopicalNeighbour
            };
            carriers.push(ForbiddenCarrier {
                meeting_id: meeting.id.clone(),
                evidence_id: evidence.id.clone(),
                state,
            });
        }
    }
    let stage = if carriers
        .iter()
        .any(|carrier| carrier.state == CarrierSourceState::CurrentAuthoritative)
    {
        ForbiddenFactStage::Answer
    } else if carriers
        .iter()
        .all(|carrier| carrier.state.is_retrieval_stage())
    {
        ForbiddenFactStage::Retrieval
    } else {
        return Err(format!(
            "{} forbidden fact {fact:?} has an unclassified current topical carrier: {:?}",
            case.id, carriers
        ));
    };
    Ok(ForbiddenFactClassification { stage, carriers })
}
