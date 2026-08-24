use app_lib::{
    database::repositories::fts::{FtsRepository, FtsSearchResult, MatchMode},
    export::build_context_markdown_with_limit,
};
use serde::Deserialize;
use sqlx::SqlitePool;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    time::Instant,
};

#[path = "fixtures/concept_lexicon.rs"]
mod concept_lexicon;
#[path = "fixtures/corpus.rs"]
mod corpus;
#[path = "fixtures/corpus_types.rs"]
mod corpus_types;

use concept_lexicon::CONCEPT_LEXICON;
use corpus_types::{
    classify_forbidden_fact, CarrierSourceState, EvaluationCase, Evidence, ForbiddenFactStage,
    Language, Meeting, MeetingState, Scope, ScopeKind,
};

const CONTEXT_BUDGET_CHARS: usize = 1_200;

const REQUIRED_CATEGORIES: [&str; 23] = [
    "reference_whatsapp",
    "exact_term",
    "number_date_name",
    "exact_number",
    "exact_date",
    "exact_name",
    "semantic_paraphrase",
    "semantic_paraphrase_pt",
    "semantic_paraphrase_en",
    "similar_topic_distractor",
    "summary_only",
    "notes_only",
    "transcript_only",
    "multi_meeting_synthesis",
    "follow_up_rewrite",
    "scope_all",
    "scope_folder",
    "scope_meeting",
    "scope_snapshot",
    "scope_today",
    "state_deleted",
    "state_dirty",
    "state_stale_derived",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvaluationPolicy {
    schema_version: u32,
    baseline: BaselinePolicy,
    gates: GatePolicy,
    lexical_policy: LexicalPolicy,
    expected_baseline: ExpectedBaseline,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BaselinePolicy {
    id: String,
    candidate_limit: usize,
    evidence_k: usize,
    normalization: String,
    query_policy: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GatePolicy {
    critical_recall_at_1: f64,
    overall_recall_at_3: f64,
    overall_recall_at_5: f64,
    evidence_recall_at_10: f64,
    source_precision: f64,
    critical_fact_coverage: f64,
    critical_forbidden_contamination: f64,
    semantic_recall_at_3_delta_points: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LexicalPolicy {
    core_term_normalization: String,
    selection_rule: String,
    portuguese_high_frequency: Vec<String>,
    english_high_frequency: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedBaseline {
    meeting_recall_at_1: Fraction,
    meeting_recall_at_3: Fraction,
    meeting_recall_at_5: Fraction,
    mrr_micros: u64,
    evidence_recall_at_10: Fraction,
    fact_coverage: Fraction,
    forbidden_contamination: Fraction,
    source_precision: Fraction,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
struct Fraction {
    numerator: usize,
    denominator: usize,
}

impl Fraction {
    fn ratio(self) -> f64 {
        if self.denominator == 0 {
            0.0
        } else {
            self.numerator as f64 / self.denominator as f64
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RetrievedEvidence {
    meeting_id: String,
    meeting_title: String,
    source_kind: String,
    evidence_id: String,
    text: String,
    context_text: String,
    folder_name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CaseOutput {
    retrieved: Vec<RetrievedEvidence>,
    emitted_source_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct CaseMetrics {
    meeting_ranks: BTreeMap<String, usize>,
    evidence_hits: usize,
    evidence_total: usize,
    fact_hits: usize,
    fact_total: usize,
    forbidden_hits: usize,
    forbidden_total: usize,
    retrieval_forbidden_hits: usize,
    retrieval_forbidden_total: usize,
    answer_forbidden_hits: usize,
    answer_forbidden_total: usize,
    source_hits: usize,
    source_total: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Metrics {
    recall_at_1: Fraction,
    recall_at_3: Fraction,
    recall_at_5: Fraction,
    mrr: f64,
    evidence_recall_at_10: Fraction,
    fact_coverage: Fraction,
    forbidden_contamination: Fraction,
    retrieval_forbidden_contamination: Fraction,
    answer_forbidden_contamination: Fraction,
    source_precision: Fraction,
    cases: BTreeMap<String, CaseMetrics>,
}

#[derive(Default)]
struct LatencyHooks(BTreeMap<&'static str, Vec<u128>>);

impl LatencyHooks {
    fn record<T>(&mut self, stage: &'static str, started: Instant, value: T) -> T {
        self.0
            .entry(stage)
            .or_default()
            .push(started.elapsed().as_micros());
        value
    }

    fn report(&self) -> String {
        self.0
            .iter()
            .map(|(stage, values)| {
                let mut values = values.clone();
                values.sort_unstable();
                let p50 = percentile(&values, 50);
                let p95 = percentile(&values, 95);
                format!("{stage}: p50={p50}us p95={p95}us n={}", values.len())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn percentile(values: &[u128], percentile: usize) -> u128 {
    values
        .get((values.len().saturating_sub(1) * percentile) / 100)
        .copied()
        .unwrap_or(0)
}

fn policy() -> EvaluationPolicy {
    serde_json::from_str(include_str!("fixtures/evaluation_policy.json"))
        .expect("evaluation policy fixture must be valid JSON")
}

async fn setup_case(case: &EvaluationCase) -> SqlitePool {
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("connect in-memory evaluation database");
    sqlx::query(
        r#"
        CREATE TABLE meetings (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            folder_id TEXT
        );
        CREATE TABLE meeting_folders (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            parent_id TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE transcripts (
            id TEXT PRIMARY KEY,
            meeting_id TEXT NOT NULL,
            transcript TEXT NOT NULL,
            timestamp TEXT NOT NULL
        );
        CREATE VIRTUAL TABLE meeting_fts USING fts5(
            meeting_id UNINDEXED,
            chunk_type UNINDEXED,
            chunk_id UNINDEXED,
            text,
            speaker UNINDEXED,
            timestamp_label UNINDEXED,
            folder_id UNINDEXED,
            folder_name,
            tokenize = 'unicode61'
        );
        "#,
    )
    .execute(&pool)
    .await
    .expect("create evaluation FTS schema");

    let folders = case
        .meetings
        .iter()
        .filter_map(|meeting| meeting.folder_id.as_deref())
        .collect::<BTreeSet<_>>();
    for folder_id in folders {
        sqlx::query(
            "INSERT INTO meeting_folders (id, name, created_at) VALUES (?1, ?2, '2026-08-01')",
        )
        .bind(folder_id)
        .bind(format!("Synthetic folder {folder_id}"))
        .execute(&pool)
        .await
        .expect("insert synthetic folder");
    }

    for meeting in &case.meetings {
        if meeting.state != MeetingState::Deleted {
            sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_id) VALUES (?1, ?2, ?3, ?3, ?4)")
                .bind(&meeting.id)
                .bind(&meeting.title)
                .bind(&meeting.meeting_date)
                .bind(&meeting.folder_id)
                .execute(&pool)
                .await
                .expect("insert synthetic meeting");
        }
        for evidence in &meeting.evidence {
            if meeting.state == MeetingState::Deleted {
                continue;
            }
            sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, folder_id, folder_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
                .bind(&meeting.id)
                .bind(&evidence.source_kind)
                .bind(&evidence.id)
                .bind(&evidence.indexed_text)
                .bind(&meeting.folder_id)
                .bind(meeting.folder_id.as_deref().unwrap_or(""))
                .execute(&pool)
                .await
                .expect("insert synthetic FTS evidence");
        }
    }
    pool
}

async fn run_current_fts(
    case: &EvaluationCase,
    baseline: &BaselinePolicy,
) -> Vec<RetrievedEvidence> {
    let pool = setup_case(case).await;
    let query = case.rewritten_query.as_deref().unwrap_or(&case.question);
    let limit = baseline.candidate_limit as u32;
    let rows = match case.scope.kind {
        ScopeKind::Snapshot | ScopeKind::Today => {
            FtsRepository::get_by_meeting_ids(&pool, &case.scope.allowed_meeting_ids, limit, limit)
                .await
                .expect("run current allow-list FTS hydration")
        }
        _ => {
            let and_rows = search_scope(&pool, case, query, limit, MatchMode::And).await;
            let claimed = and_rows.iter().map(result_identity).collect::<HashSet<_>>();
            let or_rows = search_scope(&pool, case, query, limit * 2, MatchMode::Or)
                .await
                .into_iter()
                .filter(|row| !claimed.contains(&result_identity(row)))
                .take(limit as usize)
                .collect::<Vec<_>>();
            interleave(and_rows, or_rows, limit as usize)
        }
    };
    rows.into_iter()
        .map(|row| {
            let text = row
                .snippet
                .replace("<mark>", "")
                .replace("</mark>", "")
                .trim_matches('.')
                .to_string();
            RetrievedEvidence {
                meeting_id: row.meeting_id,
                meeting_title: row.meeting_title,
                source_kind: row.chunk_type,
                evidence_id: row.chunk_id,
                text,
                context_text: row.snippet,
                folder_name: row.folder_name,
            }
        })
        .collect()
}

async fn search_scope(
    pool: &SqlitePool,
    case: &EvaluationCase,
    query: &str,
    limit: u32,
    mode: MatchMode,
) -> Vec<FtsSearchResult> {
    match case.scope.kind {
        ScopeKind::All => FtsRepository::search_with_mode(pool, query, limit, None, mode).await,
        ScopeKind::Folder => {
            FtsRepository::search_with_folder_ids(
                pool,
                query,
                limit,
                &[case.scope.folder_id.clone().expect("folder scope ID")],
                mode,
            )
            .await
        }
        ScopeKind::Meeting => {
            // Meeting scope permits several meetings, so the baseline must rank
            // inside that permitted set rather than pin one meeting.
            FtsRepository::search_with_mode(pool, query, limit * 4, None, mode)
                .await
                .map(|rows| {
                    rows.into_iter()
                        .filter(|row| case.scope.allowed_meeting_ids.contains(&row.meeting_id))
                        .take(limit as usize)
                        .collect()
                })
        }
        ScopeKind::Snapshot | ScopeKind::Today => unreachable!(),
    }
    .expect("run current FTS baseline")
}

fn result_identity(result: &FtsSearchResult) -> (String, String, String) {
    (
        result.meeting_id.clone(),
        result.chunk_type.clone(),
        result.chunk_id.clone(),
    )
}

fn interleave(
    first: Vec<FtsSearchResult>,
    second: Vec<FtsSearchResult>,
    limit: usize,
) -> Vec<FtsSearchResult> {
    let mut lists = [first.into_iter(), second.into_iter()];
    let mut output = Vec::new();
    while output.len() < limit {
        let before = output.len();
        for list in &mut lists {
            if let Some(row) = list.next() {
                output.push(row);
            }
            if output.len() == limit {
                break;
            }
        }
        if output.len() == before {
            break;
        }
    }
    output
}

fn score_case(case: &EvaluationCase, output: &CaseOutput, evidence_k: usize) -> CaseMetrics {
    let retrieved = &output.retrieved;
    let mut meeting_ranks = BTreeMap::new();
    let mut seen_meetings = HashSet::new();
    for evidence in retrieved {
        if seen_meetings.insert(&evidence.meeting_id) {
            meeting_ranks.insert(evidence.meeting_id.clone(), seen_meetings.len());
        }
    }
    let retained = retrieved.iter().take(evidence_k).collect::<Vec<_>>();
    let retained_ids = retained
        .iter()
        .map(|evidence| evidence.evidence_id.as_str())
        .collect::<HashSet<_>>();
    let retained_text = retained
        .iter()
        .map(|evidence| evidence.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let evidence_hits = case
        .required_evidence_ids
        .iter()
        .filter(|id| retained_ids.contains(id.as_str()))
        .count();
    let fact_hits = case
        .required_facts
        .iter()
        .filter(|fact| retained_text.contains(&fact.to_lowercase()))
        .count();
    let mut forbidden_hits = 0;
    let mut retrieval_forbidden_hits = 0;
    let mut retrieval_forbidden_total = 0;
    let mut answer_forbidden_hits = 0;
    let mut answer_forbidden_total = 0;
    for fact in &case.forbidden_facts {
        let hit = retained_text.contains(&fact.to_lowercase());
        forbidden_hits += usize::from(hit);
        match classify_forbidden_fact(case, fact).expect("forbidden fact carrier classification") {
            classification if classification.stage == ForbiddenFactStage::Retrieval => {
                retrieval_forbidden_total += 1;
                retrieval_forbidden_hits += usize::from(hit);
            }
            _ => {
                answer_forbidden_total += 1;
                answer_forbidden_hits += usize::from(hit);
            }
        }
    }
    let context_ids = production_retained_ids(retrieved)
        .into_iter()
        .collect::<HashSet<_>>();
    CaseMetrics {
        meeting_ranks,
        evidence_hits,
        evidence_total: case.required_evidence_ids.len(),
        fact_hits,
        fact_total: case.required_facts.len(),
        forbidden_hits,
        forbidden_total: case.forbidden_facts.len(),
        retrieval_forbidden_hits,
        retrieval_forbidden_total,
        answer_forbidden_hits,
        answer_forbidden_total,
        source_hits: output
            .emitted_source_ids
            .iter()
            .filter(|source_id| context_ids.contains(*source_id))
            .count(),
        source_total: output.emitted_source_ids.len(),
    }
}

fn as_fts_result(evidence: &RetrievedEvidence) -> FtsSearchResult {
    FtsSearchResult {
        meeting_id: evidence.meeting_id.clone(),
        meeting_title: evidence.meeting_title.clone(),
        chunk_type: evidence.source_kind.clone(),
        chunk_id: evidence.evidence_id.clone(),
        snippet: evidence.context_text.clone(),
        speaker: None,
        timestamp_label: None,
        folder_id: None,
        folder_name: evidence.folder_name.clone(),
        rank: 0.0,
    }
}

fn stable_source_id(evidence: &RetrievedEvidence) -> String {
    serde_json::to_string(&(
        &evidence.meeting_id,
        &evidence.source_kind,
        &evidence.evidence_id,
    ))
    .expect("evaluation evidence identity is serializable")
}

fn production_retained_ids(retrieved: &[RetrievedEvidence]) -> Vec<String> {
    let rows = retrieved.iter().map(as_fts_result).collect::<Vec<_>>();
    build_context_markdown_with_limit(&rows, CONTEXT_BUDGET_CHARS).retained_evidence_ids
}

fn production_case_output(retrieved: Vec<RetrievedEvidence>) -> CaseOutput {
    let emitted_source_ids = production_retained_ids(&retrieved);
    CaseOutput {
        retrieved,
        emitted_source_ids,
    }
}

fn aggregate(
    cases: &[EvaluationCase],
    results: &BTreeMap<String, CaseOutput>,
    evidence_k: usize,
) -> Metrics {
    let mut metrics = Metrics::default();
    let mut mrr_sum = 0.0;
    for case in cases {
        let empty = CaseOutput::default();
        let case_metrics = score_case(case, results.get(&case.id).unwrap_or(&empty), evidence_k);
        for meeting_id in &case.expected_meeting_ids {
            let rank = case_metrics.meeting_ranks.get(meeting_id).copied();
            metrics.recall_at_1.denominator += 1;
            metrics.recall_at_3.denominator += 1;
            metrics.recall_at_5.denominator += 1;
            metrics.recall_at_1.numerator += usize::from(rank.is_some_and(|rank| rank <= 1));
            metrics.recall_at_3.numerator += usize::from(rank.is_some_and(|rank| rank <= 3));
            metrics.recall_at_5.numerator += usize::from(rank.is_some_and(|rank| rank <= 5));
        }
        let best_rank = case
            .expected_meeting_ids
            .iter()
            .filter_map(|meeting_id| case_metrics.meeting_ranks.get(meeting_id))
            .min()
            .copied();
        mrr_sum += best_rank.map(|rank| 1.0 / rank as f64).unwrap_or(0.0);
        metrics.evidence_recall_at_10.numerator += case_metrics.evidence_hits;
        metrics.evidence_recall_at_10.denominator += case_metrics.evidence_total;
        metrics.fact_coverage.numerator += case_metrics.fact_hits;
        metrics.fact_coverage.denominator += case_metrics.fact_total;
        metrics.forbidden_contamination.numerator += case_metrics.forbidden_hits;
        metrics.forbidden_contamination.denominator += case_metrics.forbidden_total;
        metrics.retrieval_forbidden_contamination.numerator +=
            case_metrics.retrieval_forbidden_hits;
        metrics.retrieval_forbidden_contamination.denominator +=
            case_metrics.retrieval_forbidden_total;
        metrics.answer_forbidden_contamination.numerator += case_metrics.answer_forbidden_hits;
        metrics.answer_forbidden_contamination.denominator += case_metrics.answer_forbidden_total;
        metrics.source_precision.numerator += case_metrics.source_hits;
        metrics.source_precision.denominator += case_metrics.source_total;
        metrics.cases.insert(case.id.clone(), case_metrics);
    }
    metrics.mrr = mrr_sum / cases.len() as f64;
    metrics
}

async fn evaluate_baseline(
    cases: &[EvaluationCase],
    policy: &EvaluationPolicy,
) -> (Metrics, BTreeMap<String, CaseOutput>, LatencyHooks) {
    let mut results = BTreeMap::new();
    let mut latencies = LatencyHooks::default();
    for case in cases {
        let started = Instant::now();
        let retrieved = run_current_fts(case, &policy.baseline).await;
        let output = production_case_output(retrieved);
        results.insert(case.id.clone(), latencies.record("fts", started, output));
    }
    let started = Instant::now();
    let metrics = aggregate(cases, &results, policy.baseline.evidence_k);
    let metrics = latencies.record("metrics_and_context", started, metrics);
    (metrics, results, latencies)
}

fn validate_corpus(cases: &[EvaluationCase]) -> Result<(), String> {
    if cases.len() < 120 {
        return Err(format!("total corpus below floor: {}/120", cases.len()));
    }
    let portuguese = cases
        .iter()
        .filter(|case| case.language == Language::Portuguese)
        .count();
    let english = cases
        .iter()
        .filter(|case| case.language == Language::English)
        .count();
    let critical = cases.iter().filter(|case| case.critical).count();
    if portuguese < 40 || english < 40 || critical < 5 {
        return Err(format!(
            "language/critical floor failed: pt={portuguese}/40 en={english}/40 critical={critical}/5"
        ));
    }
    for category in REQUIRED_CATEGORIES {
        let count = cases
            .iter()
            .filter(|case| case.categories.iter().any(|value| value == category))
            .count();
        if count < 15 {
            return Err(format!("category {category} below floor: {count}/15"));
        }
    }
    for case in cases {
        if case.id.trim().is_empty()
            || case.question.trim().is_empty()
            || case.scope.allowed_meeting_ids.is_empty()
            || case.meetings.is_empty()
            || case.expected_meeting_ids.is_empty()
            || case.required_evidence_ids.is_empty()
            || case.required_facts.is_empty()
            || case.answer_mode.trim().is_empty()
        {
            return Err(format!("{} is missing a required fixture field", case.id));
        }
        for (before, after) in &case.order_constraints {
            if !case.expected_meeting_ids.contains(before)
                || !case.expected_meeting_ids.contains(after)
            {
                return Err(format!("{} has an invalid order constraint", case.id));
            }
        }
    }
    Ok(())
}

fn validate_private_safe(cases: &[EvaluationCase]) -> Result<(), String> {
    let forbidden_markers = [
        "c:\\users\\",
        "/users/",
        "@gmail.",
        "@outlook.",
        "sk-",
        "api_key",
        "bearer ",
        "oneDrive\\",
    ];
    for case in cases {
        let mut values = vec![
            case.id.as_str(),
            case.question.as_str(),
            case.answer_mode.as_str(),
        ];
        values.extend(case.history.iter().map(String::as_str));
        values.extend(case.rewritten_query.iter().map(String::as_str));
        values.extend(case.required_facts.iter().map(String::as_str));
        values.extend(case.forbidden_facts.iter().map(String::as_str));
        for meeting in &case.meetings {
            values.push(&meeting.id);
            values.push(&meeting.title);
            for evidence in &meeting.evidence {
                values.push(&evidence.id);
                values.push(&evidence.indexed_text);
                values.push(&evidence.authoritative_text);
            }
        }
        for value in values {
            let lowercase = value.to_lowercase();
            if forbidden_markers
                .iter()
                .any(|marker| lowercase.contains(&marker.to_lowercase()))
            {
                return Err(format!("private marker in fixture case {}", case.id));
            }
        }
    }
    Ok(())
}

fn validate_baseline_expectations(
    cases: &[EvaluationCase],
    metrics: &Metrics,
    results: &BTreeMap<String, CaseOutput>,
) -> Result<(), String> {
    for case in cases {
        let scored = metrics.cases.get(&case.id).expect("case metrics");
        let retrieved = &results.get(&case.id).expect("case results").retrieved;
        if retrieved.iter().any(|evidence| {
            !case
                .scope
                .allowed_meeting_ids
                .contains(&evidence.meeting_id)
        }) {
            return Err(format!("{} returned an out-of-scope meeting", case.id));
        }
        for (before, after) in &case.order_constraints {
            let before_rank = scored
                .meeting_ranks
                .get(before)
                .copied()
                .unwrap_or(usize::MAX);
            let after_rank = scored
                .meeting_ranks
                .get(after)
                .copied()
                .unwrap_or(usize::MAX);
            if before_rank >= after_rank {
                return Err(format!("{} violated an expected meeting order", case.id));
            }
        }
        for meeting in &case.meetings {
            let was_retrieved = retrieved
                .iter()
                .any(|evidence| evidence.meeting_id == meeting.id);
            match meeting.state {
                MeetingState::Deleted if was_retrieved => {
                    return Err(format!("{} returned a deleted candidate", case.id));
                }
                MeetingState::Dirty if !was_retrieved => {
                    return Err(format!("{} lost dirty-meeting lexical fallback", case.id));
                }
                MeetingState::StaleDerived if !was_retrieved || scored.forbidden_hits == 0 => {
                    return Err(format!(
                        "{} no longer exposes the pinned stale-derived baseline",
                        case.id
                    ));
                }
                _ => {}
            }
        }
        let is_reference = case
            .categories
            .iter()
            .any(|value| value == "reference_whatsapp");
        let is_semantic = case
            .categories
            .iter()
            .any(|value| value == "semantic_paraphrase");
        let is_exact = case.categories.iter().any(|value| value == "exact_term");
        if case.id == "fixture-whatsapp-retention" {
            let retained = &results.get(&case.id).expect("reference results").retrieved;
            let text = retained
                .iter()
                .map(|evidence| evidence.text.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            let expected_rank = scored
                .meeting_ranks
                .get(&case.expected_meeting_ids[0])
                .copied();
            if !matches!(expected_rank, Some(rank) if rank <= 3)
                || scored.evidence_hits >= scored.evidence_total
                || scored.fact_hits >= scored.fact_total
                || text.contains("dias 1, 3, 7, 10 e 15")
                || text.contains("unidades mpv enviam boas-vindas")
                || !(text.contains("3 dias") && text.contains("4 dias"))
            {
                return Err(format!(
                    "{} no longer reproduces the observed production failure shape: \
                     meeting rank passes or nearly passes while Evidence Recall and fact \
                     coverage fail on the complete schedule and MPV distinction, with \
                     superseded 3/4-day fragments surfacing instead",
                    case.id
                ));
            }
        } else if is_reference || is_semantic {
            let best_rank = case
                .expected_meeting_ids
                .iter()
                .filter_map(|id| scored.meeting_ranks.get(id))
                .min()
                .copied();
            let under_served = best_rank.map_or(true, |rank| rank > 3)
                || scored.evidence_hits < scored.evidence_total
                || scored.fact_hits < scored.fact_total;
            if !under_served {
                let order = retrieved
                    .iter()
                    .map(|evidence| format!("{}/{}", evidence.meeting_id, evidence.evidence_id))
                    .collect::<Vec<_>>()
                    .join(" | ");
                return Err(format!(
                    "{} reference/semantic case is no longer under-served: best_rank={:?} \
                     evidence={}/{} facts={}/{} order=[{}]",
                    case.id,
                    best_rank,
                    scored.evidence_hits,
                    scored.evidence_total,
                    scored.fact_hits,
                    scored.fact_total,
                    order
                ));
            }
        } else if is_exact {
            if case
                .expected_meeting_ids
                .iter()
                .any(|id| scored.meeting_ranks.get(id).map_or(true, |rank| *rank > 3))
                || scored.evidence_hits != scored.evidence_total
                || scored.fact_hits != scored.fact_total
            {
                return Err(format!(
                    "{} exact/name/number baseline regression: ranks={:?} evidence={}/{} facts={}/{} results={:?}",
                    case.id,
                    scored.meeting_ranks,
                    scored.evidence_hits,
                    scored.evidence_total,
                    scored.fact_hits,
                    scored.fact_total,
                    results.get(&case.id)
                ));
            }
        }
    }
    Ok(())
}

fn validate_quality_gates(
    cases: &[EvaluationCase],
    metrics: &Metrics,
    gates: &GatePolicy,
) -> Result<(), String> {
    let critical_cases = cases
        .iter()
        .filter(|case| case.critical)
        .collect::<Vec<_>>();
    let critical_expected = critical_cases
        .iter()
        .flat_map(|case| case.expected_meeting_ids.iter().map(move |id| (*case, id)))
        .collect::<Vec<_>>();
    let critical_recall_1 = critical_expected
        .iter()
        .filter(|(case, id)| {
            metrics.cases[&case.id]
                .meeting_ranks
                .get(*id)
                .is_some_and(|rank| *rank == 1)
        })
        .count() as f64
        / critical_expected.len() as f64;
    if critical_recall_1 < gates.critical_recall_at_1 {
        return Err("critical Recall@1 gate failed".to_string());
    }
    if metrics.recall_at_3.ratio() < gates.overall_recall_at_3 {
        return Err("overall Recall@3 gate failed".to_string());
    }
    if metrics.recall_at_5.ratio() < gates.overall_recall_at_5 {
        return Err("overall Recall@5 gate failed".to_string());
    }
    if metrics.evidence_recall_at_10.ratio() < gates.evidence_recall_at_10 {
        return Err("Evidence Recall@10 gate failed".to_string());
    }
    if metrics.source_precision.ratio() < gates.source_precision {
        return Err("retained-source precision gate failed".to_string());
    }
    for case in critical_cases {
        let scored = &metrics.cases[&case.id];
        if scored.fact_hits as f64 / (scored.fact_total as f64) < gates.critical_fact_coverage {
            return Err(format!("{} critical fact coverage gate failed", case.id));
        }
        if scored.retrieval_forbidden_total > 0
            && scored.retrieval_forbidden_hits as f64 / scored.retrieval_forbidden_total as f64
                > gates.critical_forbidden_contamination
        {
            return Err(format!(
                "{} critical retrieval-stage contamination gate failed",
                case.id
            ));
        }
    }
    Ok(())
}

fn oracle_results(cases: &[EvaluationCase]) -> BTreeMap<String, CaseOutput> {
    cases
        .iter()
        .map(|case| {
            let evidence_by_id = case
                .meetings
                .iter()
                .flat_map(|meeting| {
                    meeting
                        .evidence
                        .iter()
                        .map(move |evidence| (evidence.id.as_str(), (meeting, evidence)))
                })
                .collect::<HashMap<_, _>>();
            let retrieved = case
                .required_evidence_ids
                .iter()
                .map(|id| {
                    let (meeting, evidence) = evidence_by_id[id.as_str()];
                    RetrievedEvidence {
                        meeting_id: meeting.id.clone(),
                        meeting_title: meeting.title.clone(),
                        source_kind: evidence.source_kind.clone(),
                        evidence_id: evidence.id.clone(),
                        text: evidence.authoritative_text.clone(),
                        context_text: evidence.authoritative_text.clone(),
                        folder_name: meeting.folder_id.clone().unwrap_or_default(),
                    }
                })
                .collect::<Vec<_>>();
            (case.id.clone(), production_case_output(retrieved))
        })
        .collect()
}

fn normalize_core_token(token: &str) -> String {
    token
        .chars()
        .map(|character| match character.to_ascii_lowercase() {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

fn core_terms(query: &str, language: &Language, policy: &LexicalPolicy) -> Vec<String> {
    let high_frequency = match language {
        Language::Portuguese => &policy.portuguese_high_frequency,
        Language::English => &policy.english_high_frequency,
    };
    let normalized = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(normalize_core_token)
        .collect::<Vec<_>>();
    let core = normalized
        .iter()
        .cloned()
        .filter(|token| !high_frequency.contains(token))
        .collect::<Vec<_>>();
    if core.is_empty() {
        normalized
    } else {
        core
    }
}

fn normalize_case_text(text: &str) -> String {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| normalize_core_token(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(normalize_core_token)
        .collect()
}

fn concept_of(token: &str) -> Option<&'static str> {
    CONCEPT_LEXICON
        .iter()
        .find(|(_, variants)| variants.contains(&token))
        .map(|(concept, _)| *concept)
}

fn concepts_of(tokens: &[String]) -> BTreeSet<&'static str> {
    tokens
        .iter()
        .filter_map(|token| concept_of(token))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd)]
enum Channel {
    Lexical,
    Concept,
    Title,
}

impl Channel {
    fn name(self) -> &'static str {
        match self {
            Channel::Lexical => "lexical",
            Channel::Concept => "concept",
            Channel::Title => "title",
        }
    }
}

/// Supervised margin check: expected evidence IDs only label which candidates
/// are targets; every score is computed from raw fixture text via inverse
/// candidate-frequency weighting on lexical and concept channels plus meeting
/// titles.
fn case_margins(case: &EvaluationCase, policy: &LexicalPolicy) -> Vec<(Channel, f64)> {
    let query_text = case.rewritten_query.as_deref().unwrap_or(&case.question);
    let high_frequency = match case.language {
        Language::Portuguese => &policy.portuguese_high_frequency,
        Language::English => &policy.english_high_frequency,
    };
    let query_terms: Vec<String> = tokenize(query_text)
        .into_iter()
        .filter(|token| !high_frequency.contains(token))
        .collect();
    let query_concepts: Vec<&'static str> = concepts_of(&query_terms).into_iter().collect();

    let mut text_sets: Vec<(BTreeSet<String>, bool)> = Vec::new();
    let mut concept_sets: Vec<(BTreeSet<&'static str>, bool)> = Vec::new();
    let mut title_sets: Vec<(BTreeSet<String>, bool)> = Vec::new();
    for meeting in &case.meetings {
        let is_target = case.expected_meeting_ids.contains(&meeting.id);
        for evidence in &meeting.evidence {
            let tokens = tokenize(&evidence.indexed_text);
            text_sets.push((tokens.iter().cloned().collect(), is_target));
            concept_sets.push((concepts_of(&tokens), is_target));
            title_sets.push((tokenize(&meeting.title).into_iter().collect(), is_target));
        }
    }

    vec![
        (Channel::Lexical, channel_margin(&query_terms, &text_sets)),
        (
            Channel::Concept,
            channel_margin(&query_concepts, &concept_sets),
        ),
        (Channel::Title, channel_margin(&query_terms, &title_sets)),
    ]
}

fn channel_margin<T>(query_units: &[T], candidates: &[(BTreeSet<T>, bool)]) -> f64
where
    T: Ord + Clone,
{
    let mut weights = BTreeMap::new();
    for unit in query_units {
        let document_frequency = candidates
            .iter()
            .filter(|(set, _)| set.contains(unit))
            .count()
            .max(1);
        weights.insert(unit.clone(), 1.0 / document_frequency as f64);
    }
    let score = |(set, _): &(BTreeSet<T>, bool)| -> f64 {
        weights
            .iter()
            .map(|(unit, weight)| if set.contains(unit) { *weight } else { 0.0 })
            .sum()
    };
    let best = |target: bool| {
        candidates
            .iter()
            .filter(|(_, is_target)| *is_target == target)
            .map(score)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    best(true) - best(false)
}

/// SUPERVISED coverage exceptions (architecture "Corpus Solvability"): a
/// distractor containing the full normalized question verbatim — or a superset
/// of its content terms — is illegal unless the expected target raw text has
/// equivalent coverage. Expected IDs label only which candidates are targets;
/// every containment decision comes from fixture text.
fn supervised_coverage_violations(case: &EvaluationCase, policy: &LexicalPolicy) -> Vec<String> {
    let query_text = case.rewritten_query.as_deref().unwrap_or(&case.question);
    let high_frequency = match case.language {
        Language::Portuguese => &policy.portuguese_high_frequency,
        Language::English => &policy.english_high_frequency,
    };
    let query_terms: Vec<String> = tokenize(query_text)
        .into_iter()
        .filter(|token| !high_frequency.contains(token))
        .collect();
    let normalized_query = normalize_case_text(query_text);
    let mut verbatim_targets = false;
    let mut verbatim_distractors = 0usize;
    let mut term_targets = false;
    let mut term_distractors = 0usize;
    for meeting in &case.meetings {
        let is_target = case.expected_meeting_ids.contains(&meeting.id);
        for evidence in &meeting.evidence {
            let normalized_candidate = normalize_case_text(&evidence.indexed_text);
            let covers_verbatim =
                !normalized_query.is_empty() && normalized_candidate.contains(&normalized_query);
            let tokens = tokenize(&evidence.indexed_text);
            let covers_terms = query_terms.iter().all(|term| tokens.contains(term));
            if is_target {
                verbatim_targets |= covers_verbatim;
                term_targets |= covers_terms;
            } else {
                verbatim_distractors += usize::from(covers_verbatim);
                term_distractors += usize::from(covers_terms);
            }
        }
    }
    let mut violations = Vec::new();
    if verbatim_distractors > 0 && !verbatim_targets {
        violations.push(format!(
            "{id} has {verbatim_distractors} verbatim-question distractor(s) without equivalent \
             target coverage",
            id = case.id
        ));
    }
    if term_distractors > 0 && !term_targets {
        violations.push(format!(
            "{} has {term_distractors} query-superset distractor(s) without equivalent target \
             coverage",
            case.id
        ));
    }
    violations
}

/// SUPERVISED shape key: `required_evidence_ids` label which evidence is the
/// expected target material; every score/hash input is still raw fixture text.
/// ponytail: digits and digit-bearing tokens collapse to "#" so `format!`
/// ordinal variants cannot inflate the count; spelled-out ordinals are not
/// collapsed (upgrade path: an ordinal-word list if template generation ever
/// returns).
fn distinct_shape_key(case: &EvaluationCase) -> String {
    use std::hash::{Hash, Hasher};
    let required: HashSet<&str> = case
        .required_evidence_ids
        .iter()
        .map(String::as_str)
        .collect();
    let mut evidence_tokens = case
        .meetings
        .iter()
        .flat_map(|meeting| meeting.evidence.iter())
        .filter(|evidence| required.contains(evidence.id.as_str()))
        .flat_map(|evidence| tokenize(&evidence.indexed_text))
        .map(collapse_numeric_token)
        .collect::<Vec<_>>();
    evidence_tokens.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    evidence_tokens.hash(&mut hasher);
    let question_tokens = tokenize(&case.question)
        .into_iter()
        .map(collapse_numeric_token)
        .collect::<Vec<_>>()
        .join(" ");
    format!("{}|{:x}", question_tokens, hasher.finish())
}

fn collapse_numeric_token(token: String) -> String {
    if token.chars().any(|c| c.is_ascii_digit()) {
        "#".to_string()
    } else {
        token
    }
}

/// ANSWER-KEY-FREE structural solvency: reads only question/scope/meeting raw
/// text and scope schema. It must never touch `expected_meeting_ids`,
/// `required_evidence_ids`, or any other target label — those belong to the
/// supervised margin/coverage/distinctness validation.
fn validate_structural_solvency(
    cases: &[EvaluationCase],
    policy: &LexicalPolicy,
) -> Result<(), String> {
    const CLONE_WALL_LIMIT: usize = 2;
    let mut seen_questions = HashMap::new();
    let mut failures = Vec::new();
    for case in cases {
        let case_result = (|| -> Result<(), String> {
            if let Some(first) =
                seen_questions.insert(normalize_case_text(&case.question), &case.id)
            {
                return Err(format!(
                    "{} duplicates the normalized question of {first}",
                    case.id
                ));
            }
            if case.meetings.len() < 2 {
                return Err(format!("{} has fewer than two meetings", case.id));
            }
            let mut titles = HashSet::new();
            let mut dates = HashSet::new();
            for meeting in &case.meetings {
                if !titles.insert(meeting.title.to_lowercase()) {
                    return Err(format!("{} repeats a meeting title", case.id));
                }
                if !dates.insert(meeting.meeting_date.clone()) {
                    return Err(format!("{} repeats a meeting date", case.id));
                }
            }

            // Scope schema contract: each scope kind carries exactly the
            // fields production resolves, and Meeting scope must permit more
            // than one meeting with its focused meeting inside that set.
            match case.scope.kind {
                ScopeKind::Meeting => {
                    if case.scope.allowed_meeting_ids.len() < 2 {
                        return Err(format!(
                            "{} meeting scope permits a single meeting",
                            case.id
                        ));
                    }
                    let focused = case.scope.meeting_id.as_deref().ok_or_else(|| {
                        format!("{} meeting scope is missing scope.meeting_id", case.id)
                    })?;
                    if !case
                        .scope
                        .allowed_meeting_ids
                        .iter()
                        .any(|id| id == focused)
                    {
                        return Err(format!(
                            "{} meeting scope focuses on {focused}, which is outside the permitted set",
                            case.id
                        ));
                    }
                    if case.scope.folder_id.is_some() {
                        return Err(format!("{} meeting scope carries a folder_id", case.id));
                    }
                }
                ScopeKind::Folder => {
                    if case.scope.folder_id.is_none() {
                        return Err(format!(
                            "{} folder scope is missing scope.folder_id",
                            case.id
                        ));
                    }
                    if case.scope.meeting_id.is_some() {
                        return Err(format!("{} folder scope carries a meeting_id", case.id));
                    }
                }
                ScopeKind::All | ScopeKind::Snapshot | ScopeKind::Today => {
                    if case.scope.folder_id.is_some() || case.scope.meeting_id.is_some() {
                        return Err(format!(
                            "{} {:?} scope carries folder/meeting selectors",
                            case.id, case.scope.kind
                        ));
                    }
                }
            }
            if let ScopeKind::Folder = case.scope.kind {
                let folder = case.scope.folder_id.as_deref().expect("folder scope id");
                if case
                    .meetings
                    .iter()
                    .all(|meeting| meeting.folder_id.as_deref() == Some(folder))
                {
                    return Err(format!(
                        "{} folder scope excludes nothing in-corpus",
                        case.id
                    ));
                }
            }

            // Answer-key-free text checks against the effective query: only
            // raw candidate counts are inspected here. Whether a covering
            // candidate is an acceptable target restatement versus an illegal
            // distractor is decided by the supervised coverage check.
            let query_text = case.rewritten_query.as_deref().unwrap_or(&case.question);
            let high_frequency = match case.language {
                Language::Portuguese => &policy.portuguese_high_frequency,
                Language::English => &policy.english_high_frequency,
            };
            let query_terms: Vec<String> = tokenize(query_text)
                .into_iter()
                .filter(|token| !high_frequency.contains(token))
                .collect();
            let normalized_query = normalize_case_text(query_text);
            // ponytail: measured maximum over this corpus is 2 (an answer plus
            // one stale/draft restatement of the topic phrase); the Task 1.2
            // defect this guards was a wall of 30 verbatim-query distractors.
            let mut full_query_candidates = 0usize;
            let mut all_term_candidates = 0usize;
            for meeting in &case.meetings {
                for evidence in &meeting.evidence {
                    let normalized_candidate = normalize_case_text(&evidence.indexed_text);
                    if !normalized_query.is_empty()
                        && normalized_candidate.contains(&normalized_query)
                    {
                        full_query_candidates += 1;
                    }
                    let tokens = tokenize(&evidence.indexed_text);
                    if query_terms.iter().all(|term| tokens.contains(term)) {
                        all_term_candidates += 1;
                    }
                }
            }
            if full_query_candidates > CLONE_WALL_LIMIT {
                return Err(format!(
                    "{id} has {full_query_candidates} candidates containing the full normalized \
                  question verbatim (clone wall, limit {CLONE_WALL_LIMIT})",
                    id = case.id
                ));
            }
            if all_term_candidates > CLONE_WALL_LIMIT {
                return Err(format!(
                    "{} has {all_term_candidates} candidates containing every query content term \
                  (clone wall, limit {CLONE_WALL_LIMIT})",
                    case.id
                ));
            }
            let is_semantic = case
                .categories
                .iter()
                .any(|value| value == "semantic_paraphrase");
            if is_semantic {
                let strongest_overlap = case
                    .meetings
                    .iter()
                    .flat_map(|meeting| meeting.evidence.iter())
                    .map(|evidence| {
                        let tokens = tokenize(&evidence.indexed_text);
                        query_terms
                            .iter()
                            .filter(|term| tokens.contains(*term))
                            .count()
                    })
                    .max()
                    .unwrap_or(0);
                if strongest_overlap > 2 {
                    return Err(format!(
                    "{id} semantic case shares {strongest_overlap} content terms between question and a candidate; paraphrase signal must be conceptual",
                    id = case.id
                ));
                }
                let nonce_tokens = tokenize(&format!(
                    "{} {}",
                    case.question,
                    case.meetings
                        .iter()
                        .flat_map(|meeting| meeting.evidence.iter())
                        .map(|evidence| evidence.indexed_text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
                if nonce_tokens.iter().any(|token| {
                    token.chars().any(|c| c.is_ascii_digit())
                        && token.chars().any(char::is_alphabetic)
                }) {
                    return Err(format!(
                    "{} semantic case relies on a letter+digit nonce token; the paraphrase relation must discriminate instead",
                    case.id
                ));
                }
            }
            Ok(())
        })();
        if let Err(message) = case_result {
            failures.push(message);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn assert_expected_snapshot(metrics: &Metrics, expected: &ExpectedBaseline) {
    assert_eq!(metrics.recall_at_1, expected.meeting_recall_at_1);
    assert_eq!(metrics.recall_at_3, expected.meeting_recall_at_3);
    assert_eq!(metrics.recall_at_5, expected.meeting_recall_at_5);
    assert_eq!(
        (metrics.mrr * 1_000_000.0).round() as u64,
        expected.mrr_micros
    );
    assert_eq!(
        metrics.evidence_recall_at_10,
        expected.evidence_recall_at_10
    );
    assert_eq!(metrics.fact_coverage, expected.fact_coverage);
    assert_eq!(
        metrics.forbidden_contamination,
        expected.forbidden_contamination
    );
    assert_eq!(metrics.source_precision, expected.source_precision);
}

fn format_fraction(name: &str, value: Fraction) -> String {
    format!(
        "{name}: {:.2}% ({}/{})",
        value.ratio() * 100.0,
        value.numerator,
        value.denominator
    )
}

fn baseline_report(
    cases: &[EvaluationCase],
    metrics: &Metrics,
    policy: &EvaluationPolicy,
    latencies: &LatencyHooks,
) -> String {
    let mut category_counts = BTreeMap::new();
    for category in REQUIRED_CATEGORIES {
        category_counts.insert(
            category,
            cases
                .iter()
                .filter(|case| case.categories.iter().any(|value| value == category))
                .count(),
        );
    }
    let critical_recall = subset_meeting_recall(cases, metrics, 1, |case| case.critical);
    let exact_recall = subset_meeting_recall(cases, metrics, 3, |case| {
        case.categories.iter().any(|value| value == "exact_term")
    });
    let semantic_recall = subset_meeting_recall(cases, metrics, 3, |case| {
        case.categories
            .iter()
            .any(|value| value == "semantic_paraphrase")
    });
    let critical_facts = subset_case_fraction(
        cases,
        metrics,
        |case| case.critical,
        |scored| (scored.fact_hits, scored.fact_total),
    );
    let critical_retrieval_contamination = subset_case_fraction(
        cases,
        metrics,
        |case| case.critical,
        |scored| {
            (
                scored.retrieval_forbidden_hits,
                scored.retrieval_forbidden_total,
            )
        },
    );
    let critical_answer_contamination = subset_case_fraction(
        cases,
        metrics,
        |case| case.critical,
        |scored| (scored.answer_forbidden_hits, scored.answer_forbidden_total),
    );
    let mut shapes = HashSet::new();
    for case in cases {
        shapes.insert(distinct_shape_key(case));
    }
    let mut lines = vec![
        format!(
            "distinct question/evidence shapes: {}/{} (floor 80%)",
            shapes.len(),
            cases.len()
        ),
        format!("baseline: {}", policy.baseline.id),
        format!(
            "corpus: total={} pt={} en={} critical={}",
            cases.len(),
            cases
                .iter()
                .filter(|case| case.language == Language::Portuguese)
                .count(),
            cases
                .iter()
                .filter(|case| case.language == Language::English)
                .count(),
            cases.iter().filter(|case| case.critical).count()
        ),
        format!("overlapping category counts: {category_counts:?}"),
        format_fraction("Meeting Recall@1", metrics.recall_at_1),
        format_fraction("Meeting Recall@3", metrics.recall_at_3),
        format_fraction("Meeting Recall@5", metrics.recall_at_5),
        format!("MRR: {:.6} ({} cases)", metrics.mrr, cases.len()),
        format_fraction("Evidence Recall@10", metrics.evidence_recall_at_10),
        format_fraction("Required-fact coverage", metrics.fact_coverage),
        format_fraction(
            "Forbidden-fact contamination",
            metrics.forbidden_contamination,
        ),
        format_fraction(
            "Retrieval-stage forbidden-fact contamination",
            metrics.retrieval_forbidden_contamination,
        ),
        format_fraction(
            "Answer-stage forbidden facts in retained baseline context (informational; not evaluated/gated in Sprint 1)",
            metrics.answer_forbidden_contamination,
        ),
        format_fraction("Citation/source precision", metrics.source_precision),
        format!("Production generic context budget: {CONTEXT_BUDGET_CHARS} Unicode characters"),
        gate_line(
            "Critical Recall@1 gate",
            critical_recall,
            policy.gates.critical_recall_at_1,
            false,
        ),
        gate_line(
            "Overall Recall@3 gate",
            metrics.recall_at_3,
            policy.gates.overall_recall_at_3,
            false,
        ),
        gate_line(
            "Overall Recall@5 gate",
            metrics.recall_at_5,
            policy.gates.overall_recall_at_5,
            false,
        ),
        gate_line(
            "Evidence Recall@10 gate",
            metrics.evidence_recall_at_10,
            policy.gates.evidence_recall_at_10,
            false,
        ),
        gate_line(
            "Citation/source precision gate",
            metrics.source_precision,
            policy.gates.source_precision,
            false,
        ),
        gate_line(
            "Critical required-fact coverage gate",
            critical_facts,
            policy.gates.critical_fact_coverage,
            false,
        ),
        gate_line(
            "Critical retrieval-stage forbidden-fact contamination gate",
            critical_retrieval_contamination,
            policy.gates.critical_forbidden_contamination,
            true,
        ),
        format_fraction(
            "Critical answer-stage forbidden facts in retained baseline context (informational; not evaluated/gated in Sprint 1)",
            critical_answer_contamination,
        ),
        gate_line("Exact/name/number no-regression", exact_recall, 1.0, false),
        format!(
            "Semantic Recall@3 baseline: {:.2}% ({}/{}) future delta gate=+{:.2} points",
            semantic_recall.ratio() * 100.0,
            semantic_recall.numerator,
            semantic_recall.denominator,
            policy.gates.semantic_recall_at_3_delta_points
        ),
        format!("normalization: {}", policy.baseline.normalization),
        format!("baseline query policy: {}", policy.baseline.query_policy),
        format!(
            "evaluated core-term normalization: {}",
            policy.lexical_policy.core_term_normalization
        ),
        format!(
            "evaluated high-frequency selection: {} PT={:?} EN={:?}",
            policy.lexical_policy.selection_rule,
            policy.lexical_policy.portuguese_high_frequency,
            policy.lexical_policy.english_high_frequency
        ),
        format!(
            "semantic future gate: +{:.0} Recall@3 percentage points over this baseline",
            policy.gates.semantic_recall_at_3_delta_points
        ),
        "latency hooks (observational; excluded from deterministic snapshot):".to_string(),
        latencies.report(),
    ];
    for case in cases {
        let expectation = if case
            .categories
            .iter()
            .any(|value| value == "reference_whatsapp")
        {
            "EXPECTED_FTS_FAILURE_REFERENCE"
        } else if case
            .categories
            .iter()
            .any(|value| value == "semantic_paraphrase")
        {
            "EXPECTED_FTS_FAILURE_SEMANTIC"
        } else {
            "EXPECTED_FTS_SUCCESS_EXACT"
        };
        lines.push(format!(
            "CASE {} categories={} status={expectation}:PASS",
            case.id,
            case.categories.join(",")
        ));
    }
    lines.join("\n")
}

fn subset_meeting_recall(
    cases: &[EvaluationCase],
    metrics: &Metrics,
    rank_limit: usize,
    include: impl Fn(&EvaluationCase) -> bool,
) -> Fraction {
    let mut fraction = Fraction::default();
    for case in cases.iter().filter(|case| include(case)) {
        for meeting_id in &case.expected_meeting_ids {
            fraction.denominator += 1;
            fraction.numerator += usize::from(
                metrics.cases[&case.id]
                    .meeting_ranks
                    .get(meeting_id)
                    .is_some_and(|rank| *rank <= rank_limit),
            );
        }
    }
    fraction
}

fn subset_case_fraction(
    cases: &[EvaluationCase],
    metrics: &Metrics,
    include: impl Fn(&EvaluationCase) -> bool,
    counts: impl Fn(&CaseMetrics) -> (usize, usize),
) -> Fraction {
    cases
        .iter()
        .filter(|case| include(case))
        .fold(Fraction::default(), |mut fraction, case| {
            let (numerator, denominator) = counts(&metrics.cases[&case.id]);
            fraction.numerator += numerator;
            fraction.denominator += denominator;
            fraction
        })
}

fn gate_line(name: &str, value: Fraction, threshold: f64, lower_is_better: bool) -> String {
    let passed = if lower_is_better {
        value.ratio() <= threshold
    } else {
        value.ratio() >= threshold
    };
    format!(
        "{name}: {} observed={:.2}% ({}/{}) gate={:.2}%",
        if passed { "PASS" } else { "FAIL" },
        value.ratio() * 100.0,
        value.numerator,
        value.denominator,
        threshold * 100.0
    )
}

#[tokio::test]
async fn current_fts_baseline_is_deterministic_and_falsifiable() {
    let started = Instant::now();
    let cases = corpus::cases();
    let load_micros = started.elapsed().as_micros();
    let policy = policy();
    assert_eq!(policy.schema_version, 1);
    validate_corpus(&cases).expect("corpus floors and schema");
    validate_private_safe(&cases).expect("private-safe synthetic corpus");

    let (first, first_results, mut latencies) = evaluate_baseline(&cases, &policy).await;
    latencies.0.insert("corpus_load", vec![load_micros]);
    validate_baseline_expectations(&cases, &first, &first_results)
        .expect("reference/semantic failures and exact no-regression");

    let (second, second_results, _) = evaluate_baseline(&cases, &policy).await;
    assert_eq!(first, second, "quality metrics must be deterministic");
    assert_eq!(
        first_results, second_results,
        "result ordering must be deterministic"
    );

    println!("{}", baseline_report(&cases, &first, &policy, &latencies));
    assert_expected_snapshot(&first, &policy.expected_baseline);
}

#[test]
fn corpus_counts_core_terms_and_private_synthetic_reference_are_pinned() {
    let cases = corpus::cases();
    let policy = policy();
    validate_corpus(&cases).unwrap();
    validate_private_safe(&cases).unwrap();
    let reference = cases
        .iter()
        .find(|case| case.id == "fixture-whatsapp-retention")
        .expect("pinned WhatsApp reference fixture");
    let authoritative = reference
        .meetings
        .iter()
        .flat_map(|meeting| meeting.evidence.iter())
        .map(|evidence| evidence.authoritative_text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(authoritative.contains("dias 1, 3, 7, 10 e 15"));
    assert!(authoritative.contains("unidades MPV enviam boas-vindas"));
    assert!(authoritative.contains("unidades não MPV iniciam confirmação cadastral"));
    assert_eq!(
        core_terms(
            &reference.question,
            &reference.language,
            &policy.lexical_policy
        ),
        ["dias", "comunicacao", "whatsapp", "fluxo", "retencao"]
    );
    let exact = cases
        .iter()
        .find(|case| case.id == "en-deleted-api-version-date")
        .expect("pinned exact-term fixture");
    let exact_terms = core_terms(&exact.question, &exact.language, &policy.lexical_policy);
    assert!(exact_terms
        .iter()
        .any(|term| term.chars().any(char::is_numeric)));
    assert!(exact_terms.iter().any(|term| term.contains("v9")));
}

#[test]
fn corpus_structural_solvency_invariants_hold_without_the_answer_key() {
    let cases = corpus::cases();
    let policy = policy();
    validate_corpus(&cases).expect("corpus floors and schema");
    validate_private_safe(&cases).expect("private-safe synthetic corpus");
    // No expected/required IDs are consulted anywhere in this check; the
    // supervised counterpart lives in
    // corpus_supervised_labels_margin_coverage_and_distinctness_hold.
    validate_structural_solvency(&cases, &policy.lexical_policy)
        .expect("answer-key-free structural solvency");
}

/// SUPERVISED invariants: `expected_meeting_ids`/`required_evidence_ids` are
/// read ONLY to label which raw-text candidates/evidence are the target.
/// Every margin, coverage decision, and shape hash is computed from fixture
/// text; labels never score, never bypass retrieval, and never widen what the
/// answer-key-free structural check accepts.
const ADMISSIBILITY_EVIDENCE_K: usize = 10;
const ADMISSIBILITY_HYDRATED_MEETINGS: usize = 5;

fn report_forbidden_fact_classifications(
    cases: &[EvaluationCase],
) -> Result<(usize, usize), String> {
    let (mut retrieval, mut answer) = (0, 0);
    for case in cases {
        for fact in &case.forbidden_facts {
            let classification = classify_forbidden_fact(case, fact)?;
            match classification.stage {
                ForbiddenFactStage::Retrieval => retrieval += 1,
                ForbiddenFactStage::Answer => answer += 1,
            }
            println!(
                "[SUPERVISED:forbidden-classification] case={} fact={fact:?} stage={} carriers=[{}]",
                case.id,
                classification.stage.label(),
                classification
                    .carriers
                    .iter()
                    .map(|carrier| format!(
                        "{}/{}:{}",
                        carrier.meeting_id,
                        carrier.evidence_id,
                        carrier.state.label()
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
    }
    println!(
        "[SUPERVISED:forbidden-classification-counts] retrieval-stage={retrieval}/{} answer-stage-deferred={answer}/{} total={}/{}",
        retrieval + answer,
        retrieval + answer,
        retrieval + answer,
        retrieval + answer
    );
    Ok((retrieval, answer))
}

fn supervised_critical_gate_admissibility(cases: &[EvaluationCase]) -> Result<(), String> {
    struct AdmDoc<'a> {
        meeting_id: &'a str,
        evidence_id: &'a str,
        is_required: bool,
        forbidden_hits: Vec<usize>,
    }
    let mut failures = Vec::new();
    for case in cases.iter().filter(|case| case.critical) {
        let classifications = case
            .forbidden_facts
            .iter()
            .map(|fact| classify_forbidden_fact(case, fact))
            .collect::<Result<Vec<_>, _>>()?;
        for (fact, classification) in case.forbidden_facts.iter().zip(&classifications) {
            if classification.stage == ForbiddenFactStage::Retrieval
                && classification.carriers.is_empty()
            {
                failures.push(format!(
                    "{} retrieval-stage fact {fact:?} has no fixture-text carrier",
                    case.id
                ));
            }
        }
        let mut docs: Vec<AdmDoc> = Vec::new();
        for meeting in case
            .meetings
            .iter()
            .filter(|meeting| meeting.state != MeetingState::Deleted)
        {
            for evidence in &meeting.evidence {
                let forbidden_hits = case
                    .forbidden_facts
                    .iter()
                    .enumerate()
                    .filter(|(index, fact)| {
                        classifications[*index].stage == ForbiddenFactStage::Retrieval
                            && evidence
                                .authoritative_text
                                .to_lowercase()
                                .contains(&fact.to_lowercase())
                    })
                    .map(|(index, _)| index)
                    .collect();
                docs.push(AdmDoc {
                    meeting_id: &meeting.id,
                    evidence_id: &evidence.id,
                    is_required: case.required_evidence_ids.contains(&evidence.id),
                    forbidden_hits,
                });
            }
        }
        let mut unachievable_by_coresidence = false;
        for (fact_index, fact) in case.forbidden_facts.iter().enumerate() {
            if classifications[fact_index].stage == ForbiddenFactStage::Answer {
                println!(
                    "[SUPERVISED:co-residence] case={} fact={fact:?} ANSWER_STAGE_EXEMPT carriers=[{}]",
                    case.id,
                    classifications[fact_index]
                        .carriers
                        .iter()
                        .map(|carrier| carrier.evidence_id.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                continue;
            }
            let carriers: Vec<&AdmDoc> = docs
                .iter()
                .filter(|d| d.forbidden_hits.contains(&fact_index))
                .collect();
            let required_co_residence = carriers.iter().any(|d| d.is_required);
            unachievable_by_coresidence |= required_co_residence;
            println!(
                "[SUPERVISED:co-residence] case={} fact={fact:?} carriers=[{}] required_co_residence={}",
                case.id,
                classifications[fact_index]
                    .carriers
                    .iter()
                    .map(|carrier| carrier.evidence_id.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                required_co_residence
            );
        }

        // Constructive existence proof under the benchmark's fixed retention
        // semantics (HYDRATED_MEETINGS / EVIDENCE_K). Meetings carrying
        // required evidence hydrate FIRST; clean alternatives follow ordered
        // by fewest forbidden-bearing documents. The pooled candidate
        // retained ordering is then constructed GLOBALLY across all hydrated
        // documents — required clean documents first, other clean documents
        // next, forbidden-bearing documents last (stable sort, so ties keep
        // carrier-first meeting order and fixture order) — so a large first
        // carrier meeting cannot push a later meeting's required document
        // out of the retained window. The verdict is derived from that
        // concrete global ordering: feasible only when it retains every
        // required document AND zero forbidden-bearing documents — which
        // additionally requires the required carrier meetings to fit inside
        // the hydration cap, the distinct required documents to fit inside
        // the retained window, and enough clean documents to fill the whole
        // window ahead of any forbidden-bearing document.
        let required_ids: std::collections::HashSet<&str> = case
            .required_evidence_ids
            .iter()
            .map(String::as_str)
            .collect();
        let meeting_docs =
            |id: &str| -> Vec<&AdmDoc> { docs.iter().filter(|d| d.meeting_id == id).collect() };
        let mut meeting_order: Vec<String> = case.meetings.iter().map(|m| m.id.clone()).collect();
        meeting_order.sort_by_key(|id| {
            (
                usize::from(
                    !meeting_docs(id)
                        .iter()
                        .any(|d| required_ids.contains(d.evidence_id)),
                ),
                meeting_docs(id)
                    .iter()
                    .filter(|d| !d.forbidden_hits.is_empty())
                    .count(),
            )
        });
        let hydrated: std::collections::HashSet<&str> = meeting_order
            .iter()
            .take(ADMISSIBILITY_HYDRATED_MEETINGS)
            .map(|id| id.as_str())
            .collect();
        let mut pool: Vec<&AdmDoc> = Vec::new();
        for id in &meeting_order {
            if hydrated.contains(id.as_str()) {
                pool.extend(meeting_docs(id));
            }
        }
        pool.sort_by_key(|d| {
            (
                usize::from(!required_ids.contains(d.evidence_id)),
                usize::from(!d.forbidden_hits.is_empty()),
            )
        });
        let required_carrier_meetings = case
            .meetings
            .iter()
            .filter(|m| {
                m.evidence
                    .iter()
                    .any(|e| required_ids.contains(e.id.as_str()))
            })
            .count();
        let required_documents = required_ids.len();
        let missing_required = required_ids
            .iter()
            .filter(|id| !pool.iter().any(|d| d.evidence_id == **id))
            .count();
        let window = ADMISSIBILITY_EVIDENCE_K.min(pool.len());
        let constructive_retained = &pool[..window];
        let retained_required = constructive_retained
            .iter()
            .filter(|d| required_ids.contains(d.evidence_id))
            .count();
        let retained_forbidden_bearing = constructive_retained
            .iter()
            .filter(|d| !d.forbidden_hits.is_empty())
            .count();
        let clean_in_pool = pool.iter().filter(|d| d.forbidden_hits.is_empty()).count();
        let forbearing_in_pool = pool.len() - clean_in_pool;
        let hydrated_pool_docs = pool.len();
        let required_le_window = required_documents <= window;
        let verdict = if unachievable_by_coresidence {
            "UNACHIEVABLE_CO_RESIDENCE"
        } else if missing_required > 0 || !required_le_window {
            "UNACHIEVABLE_REQUIRED_OUTSIDE_WINDOW"
        } else if retained_required == required_documents && retained_forbidden_bearing == 0 {
            "FEASIBLE_BY_ORDERING"
        } else {
            // The best ordering still retains a forbidden-bearing document:
            // with fewer clean documents than the retained window, every
            // ordering does.
            "UNACHIEVABLE_HYDRATION_WINDOW"
        };
        println!(
            "[SUPERVISED:evidence-admissibility] case={} verdict={verdict} \
             required_carrier_meetings={required_carrier_meetings}/{ADMISSIBILITY_HYDRATED_MEETINGS} \
             required_documents={required_documents} required_le_window={required_le_window} \
             required_missing_from_pool={missing_required} hydrated_pool_docs={hydrated_pool_docs} \
             retained_window={window} clean_docs_in_pool={clean_in_pool} \
             forbidden_bearing_docs={forbearing_in_pool} \
             constructive_retained_required={retained_required}/{required_documents} \
             constructive_retained_forbidden={retained_forbidden_bearing}",
            case.id
        );
        if verdict != "FEASIBLE_BY_ORDERING" {
            failures.push(format!("{} retrieval-stage gate is {verdict}", case.id));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

#[test]
fn corpus_supervised_labels_margin_coverage_and_distinctness_hold() {
    let cases = corpus::cases();
    let policy = policy();
    let mut channel_wins = BTreeMap::new();
    let mut supervised_violations = Vec::new();
    for case in &cases {
        let margins = case_margins(case, &policy.lexical_policy);
        let winning = margins
            .iter()
            .copied()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .expect("margin channels present");
        println!(
            "MARGIN case={:40} channel={:8} margin={:+.3}  lexical={:+.3} concept={:+.3} title={:+.3}",
            case.id,
            winning.0.name(),
            winning.1,
            margins[0].1,
            margins[1].1,
            margins[2].1
        );
        if winning.1 <= 0.0 {
            supervised_violations.push(format!(
                "[SUPERVISED:margin] {} target never beats its strongest distractor on any channel ({margins:?})",
                case.id
            ));
        }
        *channel_wins.entry(winning.0.name()).or_insert(0usize) += 1;
        for violation in supervised_coverage_violations(case, &policy.lexical_policy) {
            supervised_violations.push(format!("[SUPERVISED:coverage] {violation}"));
        }
    }
    println!("winning channel distribution: {channel_wins:?}");

    let (retrieval_facts, answer_facts) =
        report_forbidden_fact_classifications(&cases).expect("fixture-derived carrier states");
    assert_eq!((retrieval_facts, answer_facts), (107, 14));
    supervised_critical_gate_admissibility(&cases)
        .expect("retrieval-stage forbidden-fact gate admissibility");

    let chaves = cases
        .iter()
        .find(|case| case.id == "pt-ref-chaves-acesso")
        .expect("chaves critical case");
    let chaves_margins = case_margins(chaves, &policy.lexical_policy);
    assert!(
        chaves_margins[0].1 > 0.0 || chaves_margins[2].1 > 0.0,
        "pt-ref-chaves-acesso must have a positive lexical/title production channel"
    );

    // Supervised distinctness: normalized question + required target evidence
    // text only (IDs label the target evidence), with numeric tokens collapsed.
    let shapes = cases
        .iter()
        .map(distinct_shape_key)
        .collect::<HashSet<_>>()
        .len();
    println!(
        "distinct question/target-evidence shapes (supervised): {shapes}/{} (floor 96)",
        cases.len()
    );
    assert!(
        shapes * 5 >= cases.len() * 4,
        "distinct shapes below 80% floor"
    );

    assert!(
        supervised_violations.is_empty(),
        "supervised label-based invariant violations:\n{}",
        supervised_violations.join("\n")
    );
}

#[test]
fn negative_rank_evidence_and_source_mutations_are_rejected() {
    let cases = corpus::cases();
    let policy = policy();
    let normal = oracle_results(&cases);
    let normal_metrics = aggregate(&cases, &normal, policy.baseline.evidence_k);
    validate_quality_gates(&cases, &normal_metrics, &policy.gates)
        .expect("unmutated oracle suite must pass");

    let critical = cases.iter().find(|case| case.critical).unwrap();
    let mut degraded_rank = normal.clone();
    let expected = &mut degraded_rank.get_mut(&critical.id).unwrap().retrieved;
    for ordinal in 0..4 {
        expected.insert(
            ordinal,
            RetrievedEvidence {
                meeting_id: format!("degraded-rank-{ordinal}"),
                meeting_title: format!("Degraded rank {ordinal}"),
                source_kind: "transcript".to_string(),
                evidence_id: format!("degraded-evidence-{ordinal}"),
                text: "synthetic distractor".to_string(),
                context_text: "synthetic distractor".to_string(),
                folder_name: String::new(),
            },
        );
    }
    let degraded_metrics = aggregate(&cases, &degraded_rank, policy.baseline.evidence_k);
    assert!(
        validate_quality_gates(&cases, &degraded_metrics, &policy.gates)
            .unwrap_err()
            .contains("critical Recall@1")
    );

    let mut removed_evidence = normal.clone();
    let expected = removed_evidence.get_mut(&critical.id).unwrap();
    let removed_source_id = stable_source_id(
        expected
            .retrieved
            .iter()
            .find(|evidence| evidence.evidence_id == critical.required_evidence_ids[0])
            .unwrap(),
    );
    expected
        .retrieved
        .retain(|evidence| evidence.evidence_id != critical.required_evidence_ids[0]);
    expected
        .emitted_source_ids
        .retain(|source_id| source_id != &removed_source_id);
    let removed_metrics = aggregate(&cases, &removed_evidence, policy.baseline.evidence_k);
    assert!(
        validate_quality_gates(&cases, &removed_metrics, &policy.gates)
            .unwrap_err()
            .contains("critical fact coverage")
    );

    let mut mismatched_source = normal;
    let output = mismatched_source.get_mut(&critical.id).unwrap();
    let omitted = RetrievedEvidence {
        meeting_id: "source-mismatch-meeting".to_string(),
        meeting_title: "Source mismatch meeting".to_string(),
        source_kind: "transcript".to_string(),
        evidence_id: "source-not-retained".to_string(),
        text: "synthetic oversized evidence".to_string(),
        context_text: "x".repeat(CONTEXT_BUDGET_CHARS),
        folder_name: String::new(),
    };
    let omitted_source_id = stable_source_id(&omitted);
    output.retrieved.push(omitted);
    assert!(!production_retained_ids(&output.retrieved).contains(&omitted_source_id));
    output.emitted_source_ids.push(omitted_source_id);
    let mismatch_metrics = aggregate(&cases, &mismatched_source, policy.baseline.evidence_k);
    assert_eq!(
        mismatch_metrics.source_precision.numerator,
        normal_metrics.source_precision.numerator
    );
    assert_eq!(
        mismatch_metrics.source_precision.denominator,
        normal_metrics.source_precision.denominator + 1
    );
    assert!(
        validate_quality_gates(&cases, &mismatch_metrics, &policy.gates)
            .unwrap_err()
            .contains("retained-source precision")
    );
}

#[test]
fn carrier_state_and_ordering_admissibility_mutations_are_rejected() {
    let reference = corpus::cases()
        .into_iter()
        .find(|case| case.id == "fixture-whatsapp-retention")
        .expect("WhatsApp reference");
    for fact in &reference.forbidden_facts {
        let classification = classify_forbidden_fact(&reference, fact).unwrap();
        assert_eq!(classification.stage, ForbiddenFactStage::Retrieval);
        assert!(!classification.carriers.is_empty());
        assert!(classification
            .carriers
            .iter()
            .all(|carrier| carrier.state == CarrierSourceState::Superseded));
    }
    supervised_critical_gate_admissibility(std::slice::from_ref(&reference))
        .expect("patched retrieval-stage facts are feasible");

    let mut trapped = reference.clone();
    trapped.required_evidence_ids.push(
        classify_forbidden_fact(&trapped, "apenas 3 dias")
            .unwrap()
            .carriers[0]
            .evidence_id
            .clone(),
    );
    assert!(supervised_critical_gate_admissibility(&[trapped]).is_err());

    let mut uncarried = reference.clone();
    for evidence in uncarried
        .meetings
        .iter_mut()
        .flat_map(|meeting| meeting.evidence.iter_mut())
    {
        evidence.indexed_text = evidence
            .indexed_text
            .replace("apenas 3 dias", "três jornadas");
        evidence.authoritative_text = evidence
            .authoritative_text
            .replace("apenas 3 dias", "três jornadas");
    }
    assert!(supervised_critical_gate_admissibility(&[uncarried]).is_err());

    for state in [MeetingState::Dirty, MeetingState::StaleDerived] {
        let mut answer_stage = reference.clone();
        let expected = answer_stage.expected_meeting_ids[0].clone();
        let target = answer_stage
            .meetings
            .iter_mut()
            .find(|meeting| meeting.id == expected)
            .unwrap();
        target.state = state;
        target.evidence[0]
            .authoritative_text
            .push_str(" Registro atual menciona apenas 3 dias.");
        target.evidence[0].indexed_text = target.evidence[0].authoritative_text.clone();
        let classification = classify_forbidden_fact(&answer_stage, "apenas 3 dias").unwrap();
        assert_eq!(classification.stage, ForbiddenFactStage::Answer);
        assert!(classification
            .carriers
            .iter()
            .any(|carrier| carrier.state == CarrierSourceState::CurrentAuthoritative));
    }

    for state in [MeetingState::Dirty, MeetingState::StaleDerived] {
        let mut differing = reference.clone();
        assert!(differing
            .expected_meeting_ids
            .iter()
            .all(|id| id != "mtg-webinar-convites"));
        let target = differing
            .meetings
            .iter_mut()
            .find(|meeting| meeting.id == "mtg-webinar-convites")
            .unwrap();
        target.state = state;
        target.evidence[0]
            .indexed_text
            .push_str(" O índice retém apenas 3 dias de antecedência neste fluxo.");
        target.evidence[0]
            .authoritative_text
            .push_str(" A régua vigente mantém apenas 3 dias de antecedência neste fluxo.");
        assert!(classify_forbidden_fact(&differing, "apenas 3 dias").is_err());
    }

    let mut indexed_only = reference;
    for evidence in indexed_only
        .meetings
        .iter_mut()
        .flat_map(|meeting| meeting.evidence.iter_mut())
    {
        evidence.indexed_text = evidence
            .indexed_text
            .replace("apenas 3 dias", "três jornadas");
        evidence.authoritative_text = evidence
            .authoritative_text
            .replace("apenas 3 dias", "três jornadas");
    }
    let expected = indexed_only.expected_meeting_ids[0].clone();
    let target = indexed_only
        .meetings
        .iter_mut()
        .find(|meeting| meeting.id == expected)
        .unwrap();
    target.state = MeetingState::StaleDerived;
    target.evidence[0]
        .indexed_text
        .push_str(" Índice antigo menciona apenas 3 dias.");
    let classification = classify_forbidden_fact(&indexed_only, "apenas 3 dias").unwrap();
    assert_eq!(classification.stage, ForbiddenFactStage::Retrieval);
    assert_eq!(classification.carriers.len(), 1);
    assert_eq!(
        classification.carriers[0].state,
        CarrierSourceState::StaleDerived
    );
}
