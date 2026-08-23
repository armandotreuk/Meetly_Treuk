// Task 1.2R corpus: hand-authored private-safe synthetic cases. Questions,
// target evidence, wrong facts, and distractors live as literal material in
// the family modules below; this file only assembles the shared schema from
// `fixtures/corpus_types.rs`.
//
// ponytail: solvability rests on two authored invariants instead of a solver —
// failing cases keep >=3 neighbours repeating one query content term while the
// target repeats none (deterministic bm25 misrank), and the margin lexicon in
// `retrieval_evaluation.rs` covers exactly these paraphrase pairs.

#[path = "corpus/follow_up.rs"]
mod follow_up;
#[path = "corpus/multi.rs"]
mod multi;
#[path = "corpus/reference_a.rs"]
mod reference_a;
#[path = "corpus/reference_b.rs"]
mod reference_b;
#[path = "corpus/reference_c.rs"]
mod reference_c;
#[path = "corpus/reference_d.rs"]
mod reference_d;
#[path = "corpus/semantic_en.rs"]
mod semantic_en;
#[path = "corpus/semantic_pt.rs"]
mod semantic_pt;
#[path = "corpus/states_deleted.rs"]
mod states_deleted;
#[path = "corpus/states_dirty.rs"]
mod states_dirty;
#[path = "corpus/states_stale.rs"]
mod states_stale;

use super::{EvaluationCase, Evidence, Language, Meeting, MeetingState, Scope, ScopeKind};

pub(super) fn cases() -> Vec<EvaluationCase> {
    let mut all = Vec::new();
    all.extend(reference_a::cases());
    all.extend(reference_b::cases());
    all.extend(reference_c::cases());
    all.extend(reference_d::cases());
    all.extend(semantic_pt::cases());
    all.extend(semantic_en::cases());
    all.extend(follow_up::cases());
    all.extend(multi::cases());
    all.extend(states_deleted::cases());
    all.extend(states_dirty::cases());
    all.extend(states_stale::cases());
    all
}

pub(super) const REFERENCE_CATEGORY: &str = "reference_whatsapp";
pub(super) const SEMANTIC_CATEGORY: &str = "semantic_paraphrase";

// Reference sibling scaffold: the decisive sections deliberately avoid the
// question vocabulary while at least three neighbours repeat one question term
// each, so bm25 deterministically misranks the target meeting.
#[allow(clippy::too_many_arguments)]
fn sibling_case(
    id: &str,
    question: &str,
    title: &str,
    date: &str,
    folder: Option<&str>,
    kind: ScopeKind,
    folder_scope: &str,
    evidence: Vec<Evidence>,
    required: &[&str],
    facts: &[&str],
    forbidden: &[&str],
    neighbours: Vec<Meeting>,
    critical: bool,
) -> EvaluationCase {
    let target_id = format!("mtg-{id}");
    let target = mtg(
        &target_id,
        title,
        date,
        folder,
        MeetingState::Current,
        evidence,
    );
    let mut ids = vec![target.id.clone()];
    ids.extend(neighbours.iter().map(|meeting| meeting.id.clone()));
    let allowed = ids.iter().map(String::as_str).collect::<Vec<_>>();
    let scope = match kind {
        ScopeKind::All => scope(ScopeKind::All, None, None, &allowed),
        ScopeKind::Folder => scope(ScopeKind::Folder, Some(folder_scope), None, &allowed),
        ScopeKind::Snapshot => scope(ScopeKind::Snapshot, None, None, &allowed),
        ScopeKind::Today => scope(ScopeKind::Today, None, None, &allowed),
        _ => unreachable!("reference siblings use all/folder/snapshot/today scopes"),
    };
    let mut meetings = vec![target];
    meetings.extend(neighbours);
    case(
        id,
        Language::Portuguese,
        question,
        &[],
        None,
        "reference_policy_lookup",
        &[REFERENCE_CATEGORY],
        critical,
        scope,
        meetings,
        &[&target_id],
        &[],
        required,
        facts,
        forbidden,
    )
}

// Semantic scaffold: the answer text is a genuine paraphrase sharing no
// content token with the question; three rivals repeat one query term each so
// the lexical baseline deterministically misranks the target meeting. Titles
// are not FTS-indexed, so a topical target title keeps the baseline failure
// while giving the margin check its discriminating channel.
fn semantic_case(
    id: &str,
    language: Language,
    question: &str,
    target_title: &str,
    detail: &str,
    summary: &str,
    forbidden: &str,
    rivals: [&str; 3],
) -> EvaluationCase {
    let pt_markers = language == Language::Portuguese;
    let prefix = if pt_markers { "mtg-pt" } else { "mtg-en" };
    let rival_dates = ["2026-05-11", "2026-06-08", "2026-07-03"];
    let rival_meetings = rivals
        .iter()
        .enumerate()
        .map(|(index, text)| {
            dm(
                &format!("{prefix}-{id}-{index:02}"),
                &format!(
                    "{} {}",
                    if pt_markers {
                        "Nota interna"
                    } else {
                        "Internal memo"
                    },
                    index + 1
                ),
                rival_dates[index],
                // Rivals share the family folder; scope selection below decides
                // whether that folder restricts anything.
                Some(if pt_markers { "geral" } else { "general" }),
                text,
            )
        })
        .collect::<Vec<_>>();
    let target_id = format!("mtg-{id}");
    let target = mtg(
        &target_id,
        target_title,
        "2026-06-20",
        Some(if pt_markers { "geral" } else { "general" }),
        MeetingState::Current,
        vec![
            ev(&format!("{id}-sumario"), "summary", summary),
            ev(&format!("{id}-detalhe"), "note", detail),
        ],
    );
    let mut allowed_ids = vec![target.id.clone()];
    for index in 0..3 {
        allowed_ids.push(format!("{prefix}-{id}-{index:02}"));
    }
    let allowed = allowed_ids.iter().map(String::as_str).collect::<Vec<_>>();
    // Portuguese cases exercise meeting scope (several permitted meetings);
    // English cases run in all-meetings scope so every rival competes.
    let kind = if pt_markers {
        scope(ScopeKind::Meeting, None, Some(target_id.as_str()), &allowed)
    } else {
        scope(ScopeKind::All, None, None, &allowed)
    };
    let detail_id = format!("{id}-detalhe");
    let summary_id = format!("{id}-sumario");
    let required = [detail_id.as_str(), summary_id.as_str()];
    let mut meetings = vec![target];
    meetings.extend(rival_meetings);
    case(
        id,
        language,
        question,
        &[],
        None,
        "semantic_lookup",
        &[
            SEMANTIC_CATEGORY,
            if pt_markers {
                "semantic_paraphrase_pt"
            } else {
                "semantic_paraphrase_en"
            },
        ],
        false,
        kind,
        meetings,
        &[&target_id],
        &[],
        &required,
        &[summary.trim_end_matches('.')],
        &[forbidden],
    )
}

fn ev(id: &str, kind: &str, text: &str) -> Evidence {
    Evidence {
        id: id.to_string(),
        source_kind: kind.to_string(),
        indexed_text: text.to_string(),
        authoritative_text: text.to_string(),
    }
}

fn mtg(
    id: &str,
    title: &str,
    date: &str,
    folder: Option<&str>,
    state: MeetingState,
    evidence: Vec<Evidence>,
) -> Meeting {
    Meeting {
        id: id.to_string(),
        title: title.to_string(),
        folder_id: folder.map(str::to_string),
        meeting_date: date.to_string(),
        state,
        evidence,
    }
}

fn dm(id: &str, title: &str, date: &str, folder: Option<&str>, text: &str) -> Meeting {
    let evidence_id = format!("{id}-ev");
    mtg(
        id,
        title,
        date,
        folder,
        MeetingState::Current,
        vec![ev(&evidence_id, "transcript", text)],
    )
}

fn stale_ev(id: &str, kind: &str, indexed: &str, authoritative: &str) -> Evidence {
    Evidence {
        id: id.to_string(),
        source_kind: kind.to_string(),
        indexed_text: indexed.to_string(),
        authoritative_text: authoritative.to_string(),
    }
}

fn scope(kind: ScopeKind, folder: Option<&str>, focused: Option<&str>, allowed: &[&str]) -> Scope {
    Scope {
        kind,
        folder_id: folder.map(str::to_string),
        meeting_id: focused.map(str::to_string),
        allowed_meeting_ids: allowed.iter().map(|id| (*id).to_string()).collect(),
    }
}

#[allow(clippy::too_many_arguments)]
fn case(
    id: &str,
    language: Language,
    question: &str,
    history: &[&str],
    rewritten: Option<&str>,
    answer_mode: &str,
    extra_categories: &[&str],
    critical: bool,
    scope: Scope,
    meetings: Vec<Meeting>,
    expected: &[&str],
    order: &[(&str, &str)],
    required: &[&str],
    facts: &[&str],
    forbidden: &[&str],
) -> EvaluationCase {
    let kinds = meetings
        .iter()
        .flat_map(|meeting| meeting.evidence.iter())
        .map(|evidence| (evidence.id.as_str(), evidence.source_kind.as_str()))
        .collect::<std::collections::HashMap<_, _>>();
    let source_kind = kinds
        .get(required[0])
        .copied()
        .expect("first required evidence exists");
    let mut categories = vec![
        "similar_topic_distractor".to_string(),
        match source_kind {
            "summary" => "summary_only",
            "note" => "notes_only",
            _ => "transcript_only",
        }
        .to_string(),
        match scope.kind {
            ScopeKind::All => "scope_all",
            ScopeKind::Folder => "scope_folder",
            ScopeKind::Meeting => "scope_meeting",
            ScopeKind::Snapshot => "scope_snapshot",
            ScopeKind::Today => "scope_today",
        }
        .to_string(),
    ];
    categories.extend(extra_categories.iter().map(|value| (*value).to_string()));
    EvaluationCase {
        id: id.to_string(),
        language,
        question: question.to_string(),
        history: history.iter().map(|value| (*value).to_string()).collect(),
        rewritten_query: rewritten.map(str::to_string),
        scope,
        meetings,
        expected_meeting_ids: expected.iter().map(|id| (*id).to_string()).collect(),
        order_constraints: order
            .iter()
            .map(|(before, after)| ((*before).to_string(), (*after).to_string()))
            .collect(),
        required_evidence_ids: required.iter().map(|id| (*id).to_string()).collect(),
        required_facts: facts.iter().map(|fact| (*fact).to_string()).collect(),
        forbidden_facts: forbidden.iter().map(|fact| (*fact).to_string()).collect(),
        answer_mode: answer_mode.to_string(),
        categories,
        critical,
    }
}
