//! Focused Task 3.2 regressions: RRF channel fusion and deterministic ties,
//! positional cross-channel dedupe with provenance preservation, the exact
//! Sprint 1 aggregation formula, support caps against long-meeting volume,
//! the deterministic depth policy, and reranker fallback/cancellation.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use super::{
    aggregate_meetings, apply_rerank, concept_coverage, coverage_regions, dedupe_candidates, fuse,
    fuse_lexical_only, rank, select_rerank_head, title_overlap, AggregationTerms, FusedEvidence,
    RankedEvidence, RankingConfig, RerankFallback, SegmentOrder, SEARCH_RERANK_DEPTH,
};
use crate::database::repositories::retrieval::{
    MeetingSource, RetrievalRepository, SourceTranscript,
};
use crate::retrieval::chunking::{chunk_meeting, ChunkerConfig, TokenizerPolicy};
use crate::retrieval::service::{
    EvidenceProvenance, LexicalMode, QueryVariantKind, RetrievalChannel, RetrievalError,
    RetrievedEvidence,
};
use crate::retrieval::worker::{LifecycleConfig, RetrievalLifecycle};

fn terms_from_title(title_overlap: HashMap<String, f64>) -> AggregationTerms {
    AggregationTerms {
        title_overlap,
        ..AggregationTerms::default()
    }
}

#[test]
fn search_purpose_uses_the_shallower_rerank_depth() {
    assert_eq!(SEARCH_RERANK_DEPTH, 25);
    assert_eq!(
        RankingConfig::for_purpose(crate::retrieval::service::RetrievalPurpose::Search)
            .rerank_depth,
        SEARCH_RERANK_DEPTH
    );
    assert_eq!(
        RankingConfig::for_purpose(crate::retrieval::service::RetrievalPurpose::Chat).rerank_depth,
        super::CHAT_RERANK_DEPTH
    );
}

// -- Candidate builders -------------------------------------------------------

fn lexical_candidate(
    id: &str,
    meeting: &str,
    kind: &str,
    start: Option<&str>,
    template: Option<&str>,
    variant: QueryVariantKind,
    mode: Option<LexicalMode>,
    rank: usize,
) -> RetrievedEvidence {
    RetrievedEvidence {
        evidence_id: id.to_string(),
        meeting_id: meeting.to_string(),
        meeting_title: format!("Title of {meeting}"),
        source_kind: kind.to_string(),
        source_start_id: start.map(str::to_string),
        source_end_id: None,
        source_template_id: template.map(str::to_string),
        heading: None,
        ordinal: 0,
        text: format!("text of {id}"),
        speaker: None,
        timestamp_label: None,
        provenance: vec![EvidenceProvenance {
            channel: RetrievalChannel::Lexical,
            variant,
            mode,
            rank,
            query_slot: 0,
        }],
        source_aliases: Vec::new(),
    }
}

fn semantic_candidate(
    id: &str,
    meeting: &str,
    kind: &str,
    start: Option<&str>,
    end: Option<&str>,
    template: Option<&str>,
    ordinal: i64,
    rank: usize,
) -> RetrievedEvidence {
    RetrievedEvidence {
        evidence_id: id.to_string(),
        meeting_id: meeting.to_string(),
        meeting_title: format!("Title of {meeting}"),
        source_kind: kind.to_string(),
        source_start_id: start.map(str::to_string),
        source_end_id: end.map(str::to_string),
        source_template_id: template.map(str::to_string),
        heading: None,
        ordinal,
        text: format!("content {id}"),
        speaker: None,
        timestamp_label: None,
        provenance: vec![EvidenceProvenance {
            channel: RetrievalChannel::Semantic,
            variant: QueryVariantKind::Original,
            mode: None,
            rank,
            query_slot: 0,
        }],
        source_aliases: Vec::new(),
    }
}

fn provenance_for(
    evidence: &RetrievedEvidence,
    channel: RetrievalChannel,
) -> Vec<(QueryVariantKind, Option<LexicalMode>, usize)> {
    evidence
        .provenance
        .iter()
        .filter(|provenance| provenance.channel == channel)
        .map(|provenance| (provenance.variant, provenance.mode, provenance.rank))
        .collect()
}

async fn migrated_pool() -> SqlitePool {
    let options = sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

/// Word-counting tokenizer stand-in for the pinned packaged tokenizer; the
/// ordering assertions below only need deterministic token counts.
struct Words;

impl TokenizerPolicy for Words {
    fn count_tokens(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }
}

#[test]
fn fuse_sums_channel_ranks_with_shipped_constants() {
    let mut both = semantic_candidate("both", "m3", "transcript", None, None, None, 0, 1);
    both.provenance.push(EvidenceProvenance {
        channel: RetrievalChannel::Lexical,
        variant: QueryVariantKind::Original,
        mode: Some(LexicalMode::Or),
        rank: 1,
        query_slot: 0,
    });
    let candidates = vec![
        both,
        semantic_candidate("sem", "m1", "transcript", None, None, None, 0, 2),
        lexical_candidate(
            "lex",
            "m2",
            "transcript",
            None,
            None,
            QueryVariantKind::Original,
            Some(LexicalMode::Or),
            1,
        ),
    ];
    let config = RankingConfig::chat();
    let fused = fuse(&candidates, &config);
    let ids: Vec<&str> = fused
        .iter()
        .map(|entry| entry.evidence.evidence_id.as_str())
        .collect();
    assert_eq!(ids, vec!["both", "sem", "lex"]);
    assert_eq!(fused[0].fused_rank, 1);
    // wv/(k+1) + wl/(k+1) vs wv/(k+2) vs wl/(k+2): only RANKS were fused, so
    // the weights come from the shipped config rather than pinned literals
    // (the constants-isolation protocol owns their values).
    let k = config.rrf_k;
    assert!(
        (fused[0].fused_score - (config.w_vector / (k + 1.0) + config.w_lexical / (k + 1.0))).abs()
            < 1e-12
    );
    assert!((fused[1].fused_score - config.w_vector / (k + 2.0)).abs() < 1e-12);
    assert!((fused[2].fused_score - config.w_lexical / (k + 2.0)).abs() < 1e-12);
}

#[test]
fn each_planner_query_slot_contributes_independently_to_fusion() {
    let config = RankingConfig::chat();
    // "sem-b" trails slot 0 at rank 2, but planner slot 1 independently
    // ranks it first. The slot-1 support is a bounded extra contribution:
    // "sem-b" overtakes "sem-a" instead of the extra support being discarded
    // because each candidate entered the channel list once.
    let mut b = semantic_candidate("sem-b", "m2", "transcript", None, None, None, 0, 2);
    b.provenance.push(EvidenceProvenance {
        channel: RetrievalChannel::Semantic,
        variant: QueryVariantKind::Original,
        mode: None,
        rank: 1,
        query_slot: 1,
    });
    let a = semantic_candidate("sem-a", "m1", "transcript", None, None, None, 0, 1);
    let fused = fuse(&[a, b], &config);
    assert_eq!(fused[0].evidence.evidence_id, "sem-b");
    assert_eq!(fused[1].evidence.evidence_id, "sem-a");
    let k = config.rrf_k;
    let expected_b = config.w_vector / (k + 2.0) + config.w_vector / (k + 1.0);
    assert!((fused[0].fused_score - expected_b).abs() < 1e-12);
    assert!((fused[1].fused_score - config.w_vector / (k + 1.0)).abs() < 1e-12);
}

#[test]
fn same_rank_in_different_planner_slots_ties_into_evidence_id_order() {
    let config = RankingConfig::chat();
    let mut slot_one = semantic_candidate("sem-z", "m1", "transcript", None, None, None, 0, 1);
    slot_one.provenance[0].query_slot = 1;
    let mut slot_two = semantic_candidate("sem-a", "m2", "transcript", None, None, None, 0, 1);
    slot_two.provenance[0].query_slot = 2;
    // Independent per-slot lists give both candidates the same position and
    // therefore the same score; the deterministic tie-break is evidence ID,
    // not whichever slot ran first.
    let fused = fuse(&[slot_two, slot_one], &config);
    assert_eq!(fused[0].evidence.evidence_id, "sem-a");
    assert_eq!(fused[1].evidence.evidence_id, "sem-z");
    assert!((fused[0].fused_score - fused[1].fused_score).abs() < 1e-12);
}

#[test]
fn fuse_ties_resolve_deterministically() {
    let config = RankingConfig {
        rrf_k: 1.0,
        w_vector: 1.0,
        w_lexical: 1.0,
        ..RankingConfig::chat()
    };
    let candidates = vec![
        semantic_candidate("a", "m1", "transcript", None, None, None, 0, 1),
        lexical_candidate(
            "b",
            "m2",
            "transcript",
            None,
            None,
            QueryVariantKind::Original,
            Some(LexicalMode::Or),
            1,
        ),
    ];
    // Both channels weight 1 with k=1: both candidates score exactly 0.5.
    for _ in 0..2 {
        let fused = fuse(&candidates, &config);
        assert!((fused[0].fused_score - fused[1].fused_score).abs() < 1e-12);
        // Equal scores resolve by evidence ID, so the order is stable.
        assert_eq!(
            fused
                .iter()
                .map(|entry| entry.evidence.evidence_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }
}

#[test]
fn lexical_only_paths_preserve_bm25_order_over_evidence_ids() {
    let candidates = vec![
        lexical_candidate(
            "z-relevant",
            "m1",
            "transcript",
            None,
            None,
            QueryVariantKind::Original,
            Some(LexicalMode::And),
            1,
        ),
        lexical_candidate(
            "a-less-relevant",
            "m2",
            "transcript",
            None,
            None,
            QueryVariantKind::Original,
            Some(LexicalMode::Or),
            2,
        ),
        semantic_candidate("semantic-only", "m3", "transcript", None, None, None, 0, 1),
    ];
    let ranked = fuse_lexical_only(&candidates, &RankingConfig::chat());
    assert_eq!(ranked[0].evidence.evidence_id, "z-relevant");
    assert_eq!(ranked[1].evidence.evidence_id, "a-less-relevant");
    assert_eq!(ranked.len(), 2);
    // The explicit forced-lexical mode uses the same ordered lexical path as
    // semantic-unavailable fallback, rather than changing the hybrid config.
    assert_eq!(ranked[0].fused_rank, 1);
}

#[test]
fn dedupe_merges_fts_segment_positionally_not_lexicographically() {
    // Chronology (audio time): z9 -> a1 -> m5. Segment IDs deliberately do
    // NOT sort chronologically, so lexicographic range containment would
    // miss the merge; positional resolution must not.
    let segment_order: SegmentOrder = [(
        "m1".to_string(),
        vec!["s-z9".to_string(), "s-a1".to_string(), "s-m5".to_string()],
    )]
    .into_iter()
    .collect();
    let window = semantic_candidate(
        "win",
        "m1",
        "transcript",
        Some("s-z9"),
        Some("s-a1"),
        None,
        0,
        1,
    );
    let fts_hit = lexical_candidate(
        "fts:a:s-a1",
        "m1",
        "transcript",
        Some("s-a1"),
        None,
        QueryVariantKind::Original,
        Some(LexicalMode::Or),
        1,
    );
    let fts_outside = lexical_candidate(
        "fts:a:s-m5",
        "m1",
        "transcript",
        Some("s-m5"),
        None,
        QueryVariantKind::Original,
        Some(LexicalMode::Or),
        2,
    );
    let deduped = dedupe_candidates(vec![window, fts_hit, fts_outside], &segment_order);
    assert_eq!(deduped.len(), 2, "absorbed FTS segment is gone");
    let merged = deduped
        .iter()
        .find(|entry| entry.evidence_id == "win")
        .expect("window kept");
    assert_eq!(
        provenance_for(merged, RetrievalChannel::Semantic),
        [(QueryVariantKind::Original, None, 1)]
    );
    assert!(provenance_for(merged, RetrievalChannel::Lexical).is_empty());
    assert_eq!(
        merged.source_aliases[0].provenance,
        vec![EvidenceProvenance {
            channel: RetrievalChannel::Lexical,
            variant: QueryVariantKind::Original,
            mode: Some(LexicalMode::Or),
            rank: 1,
            query_slot: 0,
        }]
    );
    assert_eq!(merged.source_aliases[0].evidence_id, "fts:a:s-a1");
    assert_eq!(
        merged.source_aliases[0].source_start_id.as_deref(),
        Some("s-a1")
    );
    assert_eq!(merged.source_aliases[0].text, "text of fts:a:s-a1");
    assert!(deduped.iter().any(|e| e.evidence_id == "fts:a:s-m5"));
    assert!(!deduped.iter().any(|e| e.evidence_id == "fts:a:s-a1"));
    assert_eq!(
        fuse_lexical_only(&deduped, &RankingConfig::chat())[0]
            .evidence
            .evidence_id,
        "win"
    );
}

#[test]
fn dedupe_range_metadata_is_validated_before_merging() {
    // Missing, unknown, and reversed ranges are non-mergeable, never a
    // panic: the FTS candidates survive standalone.
    let segment_order: SegmentOrder =
        [("m1".to_string(), vec!["s-1".to_string(), "s-2".to_string()])]
            .into_iter()
            .collect();
    let missing_range = semantic_candidate(
        "win-missing",
        "m1",
        "transcript",
        Some("s-1"),
        None,
        None,
        0,
        1,
    );
    let unknown_range = semantic_candidate(
        "win-unknown",
        "m1",
        "transcript",
        Some("s-1"),
        Some("s-unknown"),
        None,
        0,
        2,
    );
    // The window's own start/end refer to positions 2..=1 in the
    // authoritative chronology (s-2 before s-1): a reversed, corrupt range.
    let reversed_range = semantic_candidate(
        "win-reversed",
        "m1",
        "transcript",
        Some("s-2"),
        Some("s-1"),
        None,
        0,
        3,
    );
    let fts_hit = lexical_candidate(
        "fts:a:s-1",
        "m1",
        "transcript",
        Some("s-1"),
        None,
        QueryVariantKind::Original,
        Some(LexicalMode::Or),
        1,
    );
    let deduped = dedupe_candidates(
        vec![missing_range, unknown_range, reversed_range, fts_hit],
        &segment_order,
    );
    assert_eq!(
        deduped.len(),
        4,
        "no window may absorb the segment: all candidates retained"
    );
    let fts_row = deduped
        .iter()
        .find(|e| e.evidence_id == "fts:a:s-1")
        .expect("FTS candidate standalone");
    assert!(provenance_for(fts_row, RetrievalChannel::Lexical).len() == 1);
    assert!(provenance_for(fts_row, RetrievalChannel::Semantic).is_empty());
}

#[test]
fn dedupe_merges_provenance_without_duplicates() {
    // Two FTS hits on different segments of the same window merge into it;
    // each contributes its own provenance entry, and no entry is recorded
    // twice.
    let segment_order: SegmentOrder =
        [("m1".to_string(), vec!["s-1".to_string(), "s-2".to_string()])]
            .into_iter()
            .collect();
    let window = semantic_candidate(
        "win",
        "m1",
        "transcript",
        Some("s-1"),
        Some("s-2"),
        None,
        0,
        1,
    );
    let fts_one = lexical_candidate(
        "fts:a:s-1",
        "m1",
        "transcript",
        Some("s-1"),
        None,
        QueryVariantKind::Original,
        Some(LexicalMode::Or),
        1,
    );
    let fts_two = lexical_candidate(
        "fts:a:s-2",
        "m1",
        "transcript",
        Some("s-2"),
        None,
        QueryVariantKind::Original,
        Some(LexicalMode::Or),
        2,
    );
    let deduped = dedupe_candidates(vec![window, fts_one, fts_two], &segment_order);
    assert_eq!(deduped.len(), 1, "both FTS rows absorbed into the window");
    let merged = &deduped[0];
    assert!(provenance_for(merged, RetrievalChannel::Lexical).is_empty());
    assert_eq!(
        merged
            .source_aliases
            .iter()
            .flat_map(|alias| alias.provenance.iter())
            .map(|provenance| (provenance.variant, provenance.mode, provenance.rank))
            .collect::<Vec<_>>(),
        [
            (QueryVariantKind::Original, Some(LexicalMode::Or), 1),
            (QueryVariantKind::Original, Some(LexicalMode::Or), 2),
        ]
    );
    assert_eq!(
        provenance_for(merged, RetrievalChannel::Lexical).len(),
        provenance_for(merged, RetrievalChannel::Lexical)
            .clone()
            .into_iter()
            .collect::<HashSet<_>>()
            .len(),
        "no duplicated provenance entries"
    );
}

#[test]
fn dedupe_retains_summary_note_rows_separately_and_drops_title_candidates() {
    // Summary/notes FTS rows cover the whole template/blob while semantic
    // documents cover individual sections/windows: no authoritative matching
    // region identity exists, so the lexical candidates are retained
    // separately with their own citable text and provenance.
    let summary_window =
        semantic_candidate("doc-sum", "m1", "summary", None, None, Some("tpl"), 0, 1);
    let notes_window = semantic_candidate("doc-notes", "m1", "notes", None, None, None, 0, 2);
    let fts_summary = lexical_candidate(
        "fts:summary:m1:tpl",
        "m1",
        "summary",
        None,
        Some("tpl"),
        QueryVariantKind::Original,
        Some(LexicalMode::And),
        1,
    );
    let fts_note = lexical_candidate(
        "fts:note:m1",
        "m1",
        "note",
        None,
        None,
        QueryVariantKind::CoreTerms,
        Some(LexicalMode::And),
        2,
    );
    let title = RetrievedEvidence {
        evidence_id: "title:m1".to_string(),
        meeting_id: "m1".to_string(),
        meeting_title: "Title of m1".to_string(),
        source_kind: "title".to_string(),
        text: "Title of m1".to_string(),
        provenance: vec![EvidenceProvenance {
            channel: RetrievalChannel::Title,
            variant: QueryVariantKind::CoreTerms,
            mode: None,
            rank: 1,
            query_slot: 0,
        }],
        ..semantic_candidate("x", "x", "transcript", None, None, None, 0, 0)
    };
    let deduped = dedupe_candidates(
        vec![summary_window, notes_window, fts_summary, fts_note, title],
        &SegmentOrder::new(),
    );
    assert_eq!(deduped.len(), 4);
    assert!(deduped.iter().all(|e| e.source_kind != "title"));
    // The lexical rows survive standalone: their citable text and
    // provenance are untouched by the semantic windows' presence.
    let fts_summary_row = deduped
        .iter()
        .find(|e| e.evidence_id == "fts:summary:m1:tpl")
        .expect("summary FTS row retained");
    assert_eq!(
        provenance_for(fts_summary_row, RetrievalChannel::Lexical),
        [(QueryVariantKind::Original, Some(LexicalMode::And), 1)]
    );
    assert!(provenance_for(fts_summary_row, RetrievalChannel::Semantic).is_empty());
    assert!(deduped.iter().any(|e| e.evidence_id == "fts:note:m1"));
}

#[test]
fn aggregate_matches_the_shipped_formula() {
    let config = RankingConfig::chat();
    let fused = vec![FusedEvidence {
        evidence: semantic_candidate("only", "m1", "transcript", None, None, None, 0, 1),
        fused_rank: 1,
        fused_score: 1.0 / 6.0,
    }];
    let mut title = HashMap::new();
    title.insert("m1".to_string(), 0.5);
    let meetings = aggregate_meetings(&fused, &terms_from_title(title.clone()), None, &config);
    assert_eq!(meetings.len(), 1);
    let meeting = &meetings[0];
    assert_eq!(meeting.rank, 1);
    assert_eq!(meeting.support, 1);
    assert!((meeting.best_fused_score - 1.0 / 6.0).abs() < 1e-12);
    // k * best_fused + alpha * (support/cap) + beta * title + delta * concept,
    // read from the shipped config so the protocol may retune the weights
    // without this formula test pinning stale literals. No concept coverage
    // is supplied here, so that term contributes zero.
    assert!((meeting.concept_coverage - 0.0).abs() < 1e-12);
    let expected =
        config.rrf_k * (1.0 / 6.0) + config.support_alpha * (1.0 / 3.0) + config.title_beta * 0.5;
    assert!((meeting.score - expected).abs() < 1e-9);
}

#[test]
fn support_cap_beats_long_meeting_volume() {
    // A long meeting with ten distinct irrelevant chunks takes lexical ranks
    // 1..=10; the correct meeting's single chunk sits behind them lexically
    // but carries the only semantic hit.
    let mut candidates: Vec<RetrievedEvidence> = (0..10)
        .map(|index| {
            lexical_candidate(
                &format!("a{index}"),
                "mtg-long",
                "transcript",
                None,
                None,
                QueryVariantKind::Original,
                Some(LexicalMode::Or),
                index + 1,
            )
        })
        .collect();
    candidates.push(lexical_candidate(
        "b-lex",
        "mtg-right",
        "transcript",
        None,
        None,
        QueryVariantKind::Original,
        Some(LexicalMode::Or),
        11,
    ));
    candidates.push(semantic_candidate(
        "b-vec",
        "mtg-right",
        "transcript",
        None,
        None,
        None,
        0,
        1,
    ));
    let fused = fuse(&candidates, &RankingConfig::chat());
    let meetings = aggregate_meetings(
        &fused,
        &AggregationTerms::default(),
        None,
        &RankingConfig::chat(),
    );
    let long = meetings
        .iter()
        .find(|m| m.meeting_id == "mtg-long")
        .expect("long meeting ranked");
    let correct = meetings
        .iter()
        .find(|m| m.meeting_id == "mtg-right")
        .expect("correct meeting ranked");
    assert_eq!(long.support, 3, "support contribution is capped at 3");
    assert!(
        correct.score > long.score,
        "duplicated volume must not beat the correct meeting"
    );
    assert_eq!(meetings[0].meeting_id, "mtg-right");
}

#[test]
fn overlapping_windows_over_one_span_do_not_inflate_support() {
    // The chunker emits OVERLAPPING transcript windows by design, and dedupe
    // merges lexical rows into windows but never merges windows with each
    // other. Counting chunks would therefore let one region of source earn
    // several units of support. Diversity is measured over distinct covered
    // regions, so it must not.
    let config = RankingConfig::chat();
    let mut segment_order = SegmentOrder::new();
    segment_order.insert(
        "m-overlap".to_string(),
        vec![
            "seg-1".to_string(),
            "seg-2".to_string(),
            "seg-3".to_string(),
            "seg-4".to_string(),
        ],
    );
    // Three windows over one span (1-2, 2-3, 3-4): transitively overlapping,
    // so one region. The rival covers a disjoint span in another meeting.
    segment_order.insert(
        "m-single".to_string(),
        vec!["only-1".to_string(), "only-2".to_string()],
    );
    let overlapping = vec![
        semantic_candidate(
            "w1",
            "m-overlap",
            "transcript",
            Some("seg-1"),
            Some("seg-2"),
            None,
            0,
            1,
        ),
        semantic_candidate(
            "w2",
            "m-overlap",
            "transcript",
            Some("seg-2"),
            Some("seg-3"),
            None,
            1,
            2,
        ),
        semantic_candidate(
            "w3",
            "m-overlap",
            "transcript",
            Some("seg-3"),
            Some("seg-4"),
            None,
            2,
            3,
        ),
        semantic_candidate(
            "s1",
            "m-single",
            "transcript",
            Some("only-1"),
            Some("only-2"),
            None,
            0,
            4,
        ),
    ];
    let regions = coverage_regions(&overlapping, &segment_order);
    assert_eq!(
        regions["w1"], regions["w2"],
        "windows sharing a segment are one region"
    );
    assert_eq!(regions["w2"], regions["w3"], "overlap is transitive");
    assert_ne!(
        regions["w1"], regions["s1"],
        "a disjoint span is its own region"
    );

    let terms = AggregationTerms {
        regions,
        ..AggregationTerms::default()
    };
    let fused = fuse(&overlapping, &config);
    let meetings = aggregate_meetings(&fused, &terms, None, &config);
    let overlap_meeting = meetings
        .iter()
        .find(|meeting| meeting.meeting_id == "m-overlap")
        .expect("overlapping meeting ranked");
    assert_eq!(
        overlap_meeting.support, 1,
        "three overlapping windows over one span are one unit of support, not three"
    );
    let single = meetings
        .iter()
        .find(|meeting| meeting.meeting_id == "m-single")
        .expect("single-window meeting ranked");
    assert_eq!(single.support, 1);
    // Duplicate coverage must not buy rank: with equal support and equal
    // title/concept terms, ordering falls to the fused score alone.
    assert!(overlap_meeting.best_fused_score >= single.best_fused_score);
}

/// The measured thin-lookalike failure shape, abstracted: one meeting whose
/// single transcript chunk tops both channels against a meeting whose answer
/// is spread across independently authored artifacts (summary + notes +
/// transcript) at lower fused ranks. Under fusion alone the thin lookalike
/// wins on `k * best_fused`; the corroboration credit must flip the order,
/// and the control configuration (`corroboration_delta = 0`) must reproduce
/// the old losing behavior so this regression fails without the policy.
#[test]
fn corroboration_prefers_documented_meeting_over_thin_lookalike() {
    let mut config = RankingConfig::chat();
    config.w_lexical = 0.5;
    config.support_alpha = 0.5;
    config.title_beta = 1.0;
    config.concept_delta = 0.0;
    let thin_chunk = semantic_candidate("thin-1", "mtg-thin", "transcript", None, None, None, 0, 1);
    let documented_summary = semantic_candidate(
        "doc-sum",
        "mtg-documented",
        "summary",
        None,
        None,
        None,
        0,
        2,
    );
    let documented_notes = lexical_candidate(
        "doc-note",
        "mtg-documented",
        "note",
        None,
        None,
        QueryVariantKind::Original,
        Some(LexicalMode::Or),
        2,
    );
    let documented_window = semantic_candidate(
        "doc-win",
        "mtg-documented",
        "transcript",
        None,
        None,
        None,
        1,
        3,
    );
    let fused = vec![
        FusedEvidence {
            evidence: thin_chunk,
            fused_rank: 1,
            fused_score: 0.238,
        },
        FusedEvidence {
            evidence: documented_summary,
            fused_rank: 2,
            fused_score: 0.125,
        },
        FusedEvidence {
            evidence: documented_notes,
            fused_rank: 3,
            fused_score: 0.1,
        },
        FusedEvidence {
            evidence: documented_window,
            fused_rank: 4,
            fused_score: 0.09,
        },
    ];
    let terms = AggregationTerms {
        concept_coverage: HashMap::from([
            ("mtg-thin".to_string(), 1.0),
            ("mtg-documented".to_string(), 1.0),
        ]),
        ..AggregationTerms::default()
    };
    let meetings = aggregate_meetings(&fused, &terms, None, &config);
    let thin = meetings
        .iter()
        .find(|meeting| meeting.meeting_id == "mtg-thin")
        .expect("thin meeting ranked");
    let documented = meetings
        .iter()
        .find(|meeting| meeting.meeting_id == "mtg-documented")
        .expect("documented meeting ranked");
    assert_eq!(thin.corroboration, 1, "one chunk is one artifact class");
    assert_eq!(
        documented.corroboration, 3,
        "transcript + notes + summary are three classes"
    );
    // Without the policy the thin lookalike wins on best-fused dominance
    // (5 * 0.238 + 0.5/3 > 5 * 0.125 + 0.5): the exact measured defect.
    let mut old_behavior = config;
    old_behavior.corroboration_delta = 0.0;
    let old = aggregate_meetings(&fused, &terms, None, &old_behavior);
    let old_thin = old
        .iter()
        .find(|meeting| meeting.meeting_id == "mtg-thin")
        .expect("thin meeting ranked");
    let old_documented = old
        .iter()
        .find(|meeting| meeting.meeting_id == "mtg-documented")
        .expect("documented meeting ranked");
    assert!(
        old_thin.score > old_documented.score,
        "control: without the corroboration credit the thin lookalike must win, \
         otherwise this regression does not pin the policy"
    );
    assert!(
        documented.score > thin.score,
        "corroboration across independently authored artifacts must outrank a \
         single-chunk lookalike"
    );
    assert_eq!(meetings[0].meeting_id, "mtg-documented");
}

#[test]
fn corroboration_requires_full_query_coverage_for_each_meeting() {
    let config = RankingConfig::chat();
    let fused = vec![
        FusedEvidence {
            evidence: semantic_candidate("target", "target", "transcript", None, None, None, 0, 1),
            fused_rank: 1,
            fused_score: 0.2,
        },
        FusedEvidence {
            evidence: semantic_candidate("decoy-t", "decoy", "transcript", None, None, None, 0, 2),
            fused_rank: 2,
            fused_score: 0.19,
        },
        FusedEvidence {
            evidence: semantic_candidate("decoy-s", "decoy", "summary", None, None, None, 1, 3),
            fused_rank: 3,
            fused_score: 0.18,
        },
    ];
    let terms = AggregationTerms {
        concept_coverage: HashMap::from([("target".to_string(), 1.0), ("decoy".to_string(), 0.5)]),
        ..AggregationTerms::default()
    };
    let scores = HashMap::from([
        ("target".to_string(), 1.0),
        ("decoy-t".to_string(), 1.0),
        ("decoy-s".to_string(), 1.0),
    ]);
    let meetings = aggregate_meetings(&fused, &terms, Some(&scores), &config);
    let decoy = meetings
        .iter()
        .find(|meeting| meeting.meeting_id == "decoy")
        .unwrap();
    assert_eq!(decoy.corroboration, 0);
    assert_eq!(meetings[0].meeting_id, "target");
}

#[test]
fn corroboration_requires_relevant_reranked_evidence_when_available() {
    let mut config = RankingConfig::chat();
    config.w_lexical = 0.5;
    config.support_alpha = 0.5;
    config.title_beta = 1.0;
    config.concept_delta = 0.0;
    let fused = vec![
        FusedEvidence {
            evidence: semantic_candidate(
                "decoy-summary",
                "decoy",
                "summary",
                None,
                None,
                None,
                0,
                1,
            ),
            fused_rank: 1,
            fused_score: 0.1825,
        },
        FusedEvidence {
            evidence: semantic_candidate("target-note", "target", "note", None, None, None, 0, 2),
            fused_rank: 2,
            fused_score: 0.22,
        },
        FusedEvidence {
            evidence: semantic_candidate(
                "decoy-transcript",
                "decoy",
                "transcript",
                None,
                None,
                None,
                1,
                3,
            ),
            fused_rank: 3,
            fused_score: 0.1,
        },
    ];
    let rerank_scores = HashMap::from([
        ("decoy-summary".to_string(), -1.0),
        ("target-note".to_string(), 1.0),
        ("decoy-transcript".to_string(), -2.0),
    ]);
    let meetings = aggregate_meetings(
        &fused,
        &AggregationTerms {
            concept_coverage: HashMap::from([("target".to_string(), 1.0)]),
            ..AggregationTerms::default()
        },
        Some(&rerank_scores),
        &config,
    );
    assert_eq!(meetings[0].meeting_id, "target");
    assert_eq!(meetings[0].corroboration, 1);
    assert_eq!(meetings[1].corroboration, 0);
}

#[test]
fn corroboration_is_volume_immune_and_excludes_selection_signals() {
    // Ten transcript chunks (the long-meeting volume shape) plus the
    // meeting's profile stay ONE artifact class: chunk volume and selection
    // signals cannot buy corroboration.
    let volume: Vec<FusedEvidence> = (0..10)
        .map(|index| FusedEvidence {
            evidence: semantic_candidate(
                &format!("v{index}"),
                "mtg-volume",
                "transcript",
                None,
                None,
                None,
                index as i64,
                index + 1,
            ),
            fused_rank: index + 1,
            fused_score: 0.2 - index as f64 * 0.01,
        })
        .collect();
    let mut with_profile = volume.clone();
    with_profile.push(FusedEvidence {
        evidence: semantic_candidate(
            "v-profile",
            "mtg-volume",
            "meeting_profile",
            None,
            None,
            None,
            0,
            11,
        ),
        fused_rank: 11,
        fused_score: 0.05,
    });
    let meetings = aggregate_meetings(
        &with_profile,
        &AggregationTerms {
            concept_coverage: HashMap::from([("mtg-volume".to_string(), 1.0)]),
            ..AggregationTerms::default()
        },
        None,
        &RankingConfig::chat(),
    );
    let volume_meeting = meetings
        .iter()
        .find(|meeting| meeting.meeting_id == "mtg-volume")
        .expect("volume meeting ranked");
    assert_eq!(
        volume_meeting.corroboration, 1,
        "duplicated transcript chunks and the profile are one class"
    );
    assert_eq!(volume_meeting.support, 3, "support stays capped");
    // Lexical note rows and semantic note-section windows are the same
    // authored artifact: one class. The credit is bounded at two classes
    // beyond the first even when profile and title signals are present.
    let mixed = vec![
        FusedEvidence {
            evidence: semantic_candidate("m-t", "mtg-mixed", "transcript", None, None, None, 0, 1),
            fused_rank: 1,
            fused_score: 0.2,
        },
        FusedEvidence {
            evidence: semantic_candidate("m-s", "mtg-mixed", "summary", None, None, None, 1, 2),
            fused_rank: 2,
            fused_score: 0.15,
        },
        FusedEvidence {
            evidence: semantic_candidate("m-n", "mtg-mixed", "notes", None, None, None, 2, 3),
            fused_rank: 3,
            fused_score: 0.1,
        },
        FusedEvidence {
            evidence: lexical_candidate(
                "m-ln",
                "mtg-mixed",
                "note",
                None,
                None,
                QueryVariantKind::Original,
                Some(LexicalMode::Or),
                1,
            ),
            fused_rank: 4,
            fused_score: 0.05,
        },
        FusedEvidence {
            evidence: semantic_candidate(
                "m-p",
                "mtg-mixed",
                "meeting_profile",
                None,
                None,
                None,
                0,
                4,
            ),
            fused_rank: 5,
            fused_score: 0.04,
        },
    ];
    let config = RankingConfig::chat();
    let terms = AggregationTerms {
        concept_coverage: HashMap::from([("mtg-mixed".to_string(), 1.0)]),
        ..AggregationTerms::default()
    };
    let meetings = aggregate_meetings(&mixed, &terms, None, &config);
    let mixed_meeting = meetings
        .iter()
        .find(|meeting| meeting.meeting_id == "mtg-mixed")
        .expect("mixed meeting ranked");
    assert_eq!(
        mixed_meeting.corroboration, 3,
        "note and notes merge; profile and title are not evidence classes"
    );
    let expected = config.rrf_k * 0.2
        + config.support_alpha * (3.0 / config.support_cap.max(1) as f64)
        + config.concept_delta
        + config.corroboration_delta * 2.0;
    assert!(
        (mixed_meeting.score - expected).abs() < 1e-9,
        "the corroboration credit is bounded at two classes beyond the first"
    );
}

#[test]
fn reranker_tail_does_not_disqualify_scored_authoritative_class() {
    let mut fused = vec![FusedEvidence {
        evidence: semantic_candidate(
            "target-positive",
            "target",
            "transcript",
            None,
            None,
            None,
            0,
            1,
        ),
        fused_rank: 1,
        fused_score: 0.2,
    }];
    fused.extend((0..60).map(|index| FusedEvidence {
        evidence: semantic_candidate(
            &format!("tail-{index}"),
            "target",
            "transcript",
            None,
            None,
            None,
            index,
            index as usize + 2,
        ),
        fused_rank: index as usize + 2,
        fused_score: 0.1 - index as f64 * 0.0001,
    }));
    fused.push(FusedEvidence {
        evidence: semantic_candidate(
            "target-summary",
            "target",
            "summary",
            None,
            None,
            None,
            0,
            62,
        ),
        fused_rank: 62,
        fused_score: 0.01,
    });
    let scores = HashMap::from([
        ("target-positive".to_string(), 1.0),
        ("target-summary".to_string(), 1.0),
    ]);
    let meeting = aggregate_meetings(
        &fused,
        &AggregationTerms {
            concept_coverage: HashMap::from([("target".to_string(), 1.0)]),
            ..AggregationTerms::default()
        },
        Some(&scores),
        &RankingConfig::chat(),
    )
    .into_iter()
    .next()
    .expect("target meeting ranked");
    assert_eq!(meeting.corroboration, 2);
}

#[test]
fn corroboration_is_global_when_other_meetings_crowd_the_support_window() {
    let config = RankingConfig::chat();
    let mut fused = vec![FusedEvidence {
        evidence: semantic_candidate(
            "target-transcript",
            "target",
            "transcript",
            None,
            None,
            None,
            0,
            1,
        ),
        fused_rank: 1,
        fused_score: 0.2,
    }];
    fused.extend((0..19).map(|index| FusedEvidence {
        evidence: semantic_candidate(
            &format!("crowd-{index}"),
            "crowd",
            "transcript",
            None,
            None,
            None,
            index,
            index as usize + 2,
        ),
        fused_rank: index as usize + 2,
        fused_score: 0.19 - index as f64 * 0.001,
    }));
    fused.extend([
        FusedEvidence {
            evidence: semantic_candidate(
                "target-summary",
                "target",
                "summary",
                None,
                None,
                None,
                0,
                21,
            ),
            fused_rank: 21,
            fused_score: 0.01,
        },
        FusedEvidence {
            evidence: semantic_candidate(
                "target-notes",
                "target",
                "notes",
                None,
                None,
                None,
                1,
                22,
            ),
            fused_rank: 22,
            fused_score: 0.009,
        },
    ]);

    let terms = AggregationTerms {
        concept_coverage: HashMap::from([("target".to_string(), 1.0)]),
        ..AggregationTerms::default()
    };
    let meetings = aggregate_meetings(&fused, &terms, None, &config);
    let target = meetings
        .iter()
        .find(|meeting| meeting.meeting_id == "target")
        .expect("target meeting ranked");
    assert_eq!(target.support, 1, "support still uses the bounded window");
    assert_eq!(target.corroboration, 3, "all fused classes remain visible");

    let mut duplicated = fused.clone();
    duplicated.extend((0..40).map(|index| FusedEvidence {
        evidence: semantic_candidate(
            &format!("duplicate-{index}"),
            "crowd",
            "transcript",
            None,
            None,
            None,
            index,
            index as usize + 23,
        ),
        fused_rank: index as usize + 23,
        fused_score: 0.008 - index as f64 * 0.00001,
    }));
    let duplicated_target = aggregate_meetings(&duplicated, &terms, None, &config)
        .into_iter()
        .find(|meeting| meeting.meeting_id == "target")
        .expect("target meeting ranked after duplication");
    assert_eq!(duplicated_target.corroboration, target.corroboration);
}

#[test]
fn concept_coverage_counts_distinct_query_concepts_across_evidence() {
    // The architecture's separate diversity measure: fraction of DISTINCT
    // query concepts present across a meeting's evidence, independent of how
    // many chunks repeat one of them.
    let core_terms: Vec<String> = vec![
        "retencao".to_string(),
        "whatsapp".to_string(),
        "cobranca".to_string(),
    ];
    let mut broad = semantic_candidate("b1", "m-broad", "transcript", None, None, None, 0, 1);
    broad.text = "fluxo de retencao por WhatsApp e cobranca".to_string();
    let mut repetitive = semantic_candidate("r1", "m-repeat", "transcript", None, None, None, 0, 2);
    repetitive.text = "retencao retencao retencao".to_string();
    let mut repetitive_two =
        semantic_candidate("r2", "m-repeat", "transcript", None, None, None, 1, 3);
    repetitive_two.text = "retencao de novo".to_string();

    let coverage = concept_coverage(&[broad, repetitive, repetitive_two], &core_terms);
    assert!((coverage["m-broad"] - 1.0).abs() < 1e-12);
    assert!(
        (coverage["m-repeat"] - 1.0 / 3.0).abs() < 1e-12,
        "repeating one concept across chunks covers one concept, not three"
    );
}

#[test]
fn aggregate_ties_resolve_by_meeting_id_like_the_evaluated_policy() {
    // Hand-built equal scores isolate the deterministic tie rule: equal
    // aggregation scores resolve by meeting ID (task-1.3-final-selection.md
    // `aggregate_meetings`); no unmeasured concept term participates.
    let fused = vec![
        FusedEvidence {
            evidence: semantic_candidate("b", "mtg-b", "transcript", None, None, None, 0, 1),
            fused_rank: 1,
            fused_score: 0.2,
        },
        FusedEvidence {
            evidence: semantic_candidate("z", "mtg-a", "transcript", None, None, None, 0, 2),
            fused_rank: 2,
            fused_score: 0.2,
        },
    ];
    let meetings = aggregate_meetings(
        &fused,
        &AggregationTerms::default(),
        None,
        &RankingConfig::chat(),
    );
    assert_eq!(meetings[0].meeting_id, "mtg-a");
    assert_eq!(meetings[1].meeting_id, "mtg-b");
}

#[test]
fn aggregate_equal_scores_put_profile_rank_before_missing_profile() {
    let fused = vec![
        FusedEvidence {
            evidence: semantic_candidate(
                "profile",
                "mtg-profile",
                "meeting_profile",
                None,
                None,
                None,
                0,
                1,
            ),
            fused_rank: 1,
            fused_score: 0.2,
        },
        FusedEvidence {
            evidence: semantic_candidate(
                "transcript",
                "mtg-no-profile",
                "transcript",
                None,
                None,
                None,
                0,
                2,
            ),
            fused_rank: 2,
            fused_score: 0.2,
        },
    ];
    let config = RankingConfig {
        support_window: 0,
        ..RankingConfig::chat()
    };
    let meetings = aggregate_meetings(&fused, &AggregationTerms::default(), None, &config);
    assert_eq!(meetings[0].meeting_id, "mtg-profile");
    assert_eq!(meetings[1].meeting_id, "mtg-no-profile");
}

#[test]
fn adaptive_depth_policy_is_deterministic() {
    let candidates: Vec<RetrievedEvidence> = (0..60)
        .map(|index| {
            semantic_candidate(
                &format!("doc{index:03}"),
                "m1",
                "transcript",
                None,
                None,
                None,
                index as i64,
                index + 1,
            )
        })
        .collect();
    let fused = fuse(&candidates, &RankingConfig::chat());
    let first = select_rerank_head(&fused, &RankingConfig::chat());
    let second = select_rerank_head(&fused, &RankingConfig::chat());
    assert_eq!(first.len(), 50);
    assert_eq!(
        first
            .iter()
            .map(|e| e.evidence.evidence_id.as_str())
            .collect::<Vec<_>>(),
        second
            .iter()
            .map(|e| e.evidence.evidence_id.as_str())
            .collect::<Vec<_>>()
    );
    // Meeting profiles are never reranked even inside the head.
    let short = vec![FusedEvidence {
        evidence: semantic_candidate("profile", "m1", "meeting_profile", None, None, None, 0, 1),
        fused_rank: 1,
        fused_score: 1.0,
    }];
    assert!(select_rerank_head(&short, &RankingConfig::chat()).is_empty());
}

#[test]
fn rerank_scores_head_the_final_order_and_recompute_meetings() {
    let candidates = vec![
        semantic_candidate("s1", "m1", "transcript", None, None, None, 0, 1),
        semantic_candidate("s2", "m2", "transcript", None, None, None, 0, 2),
        lexical_candidate(
            "l1",
            "m3",
            "transcript",
            None,
            None,
            QueryVariantKind::Original,
            Some(LexicalMode::Or),
            1,
        ),
    ];
    let fused = fuse(&candidates, &RankingConfig::chat());
    let mut scores = HashMap::new();
    scores.insert("s2".to_string(), 0.9_f32);
    scores.insert("s1".to_string(), 0.1);
    let (evidence, meetings) = apply_rerank(
        &fused,
        &scores,
        &AggregationTerms::default(),
        &RankingConfig::chat(),
    );
    // Scored evidence heads the order (score desc, fused-rank tie-break);
    // unscored evidence keeps its fused position.
    let ids: Vec<&str> = evidence_ids(&evidence);
    assert_eq!(ids, vec!["s2", "s1", "l1"]);
    assert_eq!(evidence[0].reranker_score, Some(0.9));
    assert!(evidence[2].reranker_score.is_none());
    // gamma = 0: the meeting order equals the fused aggregation order.
    assert_eq!(meetings.len(), 3);
}

fn evidence_ids(evidence: &[RankedEvidence]) -> Vec<&str> {
    evidence
        .iter()
        .map(|entry| entry.evidence.evidence_id.as_str())
        .collect()
}

#[test]
fn late_cancellation_after_inference_returns_no_outcome() {
    // The reranker inference has returned scores, but the request was
    // cancelled in the meantime: no outcome may be applied or returned.
    let candidates = vec![
        semantic_candidate("s1", "m1", "transcript", None, None, None, 0, 1),
        semantic_candidate("s2", "m2", "transcript", None, None, None, 0, 2),
    ];
    let fused = fuse(&candidates, &RankingConfig::chat());
    let head = select_rerank_head(&fused, &RankingConfig::chat());
    // Scores are positional over the head order [s1, s2]: s2 must win.
    let scores = vec![0.1_f32, 0.9];
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = super::assemble_scored_outcome(
        &fused,
        &head,
        &scores,
        &AggregationTerms::default(),
        &RankingConfig::chat(),
        &cancel,
        vec![],
        vec![],
        "pergunta",
        false,
        vec![],
    );
    assert!(matches!(outcome, Err(RetrievalError::Cancelled)));
    // Without cancellation the same assembly yields the scored outcome.
    let outcome = super::assemble_scored_outcome(
        &fused,
        &head,
        &scores,
        &AggregationTerms::default(),
        &RankingConfig::chat(),
        &CancellationToken::new(),
        vec![],
        vec![],
        "pergunta",
        false,
        vec![],
    )
    .expect("uncancelled assembly must produce the outcome");
    assert!(outcome.reranker_used);
    assert_eq!(outcome.evidence[0].evidence.evidence_id, "s2");
    assert_eq!(outcome.core_terms.len(), 0);
}

#[tokio::test]
async fn rank_falls_back_to_fused_order_when_reranker_unavailable() {
    let pool = migrated_pool().await;
    let lifecycle = RetrievalLifecycle::new(LifecycleConfig::testing(
        Arc::new(|| false),
        Arc::new(|| Err("simulated bundle unavailability".to_string())),
    ));
    let candidates = vec![
        semantic_candidate("s1", "m1", "transcript", None, None, None, 0, 1),
        lexical_candidate(
            "l1",
            "m1",
            "transcript",
            None,
            None,
            QueryVariantKind::Original,
            Some(LexicalMode::And),
            1,
        ),
    ];
    let outcome = rank(
        &lifecycle,
        &pool,
        candidates,
        "question",
        vec![],
        &RankingConfig::chat(),
        &CancellationToken::new(),
    )
    .await
    .expect("reranker unavailability must degrade, not fail");
    assert!(!outcome.reranker_used);
    assert_eq!(outcome.rerank_fallback, Some(RerankFallback::Unavailable));
    // The fused ordering is preserved: fused ranks stay ascending.
    for (position, entry) in outcome.evidence.iter().enumerate() {
        assert_eq!(entry.fused_rank, position + 1);
    }
    assert_eq!(outcome.rerank_depth, 2);
    pool.close().await;
}

#[tokio::test]
async fn cancellation_during_model_work_never_returns_a_fallback_outcome() {
    // The real `rank` fallback branch, not the private assembly helper:
    // cancellation lands DURING model work (the loader cancels as its side
    // effect, then fails), so the request takes the Unavailable fallback
    // path. Cancellation must still win - a cancelled request may never
    // publish a fallback outcome.
    let pool = migrated_pool().await;
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let lifecycle = RetrievalLifecycle::new(LifecycleConfig::testing(
        Arc::new(|| false),
        Arc::new(move || {
            trigger.cancel();
            Err("cancelled while loading".to_string())
        }),
    ));
    let candidates = vec![semantic_candidate(
        "s1",
        "m1",
        "transcript",
        None,
        None,
        None,
        0,
        1,
    )];
    let outcome = rank(
        &lifecycle,
        &pool,
        candidates,
        "question",
        vec![],
        &RankingConfig::chat(),
        &cancel,
    )
    .await;
    assert!(
        matches!(outcome, Err(RetrievalError::Cancelled)),
        "a cancelled request must not publish a fallback outcome"
    );
    pool.close().await;
}

#[tokio::test]
async fn rank_cancellation_propagates_and_never_falls_back() {
    let pool = migrated_pool().await;
    let lifecycle = RetrievalLifecycle::new(LifecycleConfig::testing(
        Arc::new(|| false),
        Arc::new(|| Err("simulated bundle unavailability".to_string())),
    ));
    let cancel = CancellationToken::new();
    cancel.cancel();
    let candidates = vec![semantic_candidate(
        "s1",
        "m1",
        "transcript",
        None,
        None,
        None,
        0,
        1,
    )];
    let outcome = rank(
        &lifecycle,
        &pool,
        candidates,
        "question",
        vec![],
        &RankingConfig::chat(),
        &cancel,
    )
    .await;
    assert!(matches!(outcome, Err(RetrievalError::Cancelled)));
    pool.close().await;
}

#[test]
fn title_only_meeting_survives_ranking_behind_evidence_meetings() {
    // Task 3.1's title channel exists so a title match never depends on
    // lexical/semantic health. Its candidates are selection signals, not
    // evidence, so dedupe drops them - but the meeting must still reach the
    // ranking on its title term, ordered behind any meeting with real fused
    // evidence.
    let config = RankingConfig::chat();
    let core_terms: Vec<String> = vec!["retrospectiva".to_string()];
    let title_only = RetrievedEvidence {
        evidence_id: "title:m-title".to_string(),
        meeting_id: "m-title".to_string(),
        meeting_title: "Retrospectiva Q3".to_string(),
        source_kind: "title".to_string(),
        text: "Retrospectiva Q3".to_string(),
        provenance: vec![EvidenceProvenance {
            channel: RetrievalChannel::Title,
            variant: QueryVariantKind::CoreTerms,
            mode: None,
            rank: 1,
            query_slot: 0,
        }],
        ..semantic_candidate("x", "x", "transcript", None, None, None, 0, 0)
    };
    let mut with_evidence =
        semantic_candidate("doc-body", "m-body", "transcript", None, None, None, 0, 1);
    with_evidence.meeting_title = "Planejamento".to_string();
    let candidates = vec![title_only, with_evidence];

    // Exactly the production order: overlap is computed before dedupe.
    let title = title_overlap(&candidates, &core_terms);
    let deduped = dedupe_candidates(candidates, &SegmentOrder::new());
    let fused = fuse(&deduped, &config);
    let meetings = aggregate_meetings(&fused, &terms_from_title(title.clone()), None, &config);

    assert!((title["m-title"] - 1.0).abs() < 1e-12);
    assert!(
        deduped.iter().all(|e| e.source_kind != "title"),
        "title candidates remain selection signals, not evidence"
    );
    let ranked: Vec<&str> = meetings
        .iter()
        .map(|meeting| meeting.meeting_id.as_str())
        .collect();
    assert_eq!(
        ranked,
        ["m-body", "m-title"],
        "the title-only meeting must rank, behind the meeting with fused evidence"
    );
    let title_meeting = meetings
        .iter()
        .find(|meeting| meeting.meeting_id == "m-title")
        .expect("title-only meeting present in the ranking");
    assert_eq!(title_meeting.support, 0);
    assert!((title_meeting.best_fused_score - 0.0).abs() < 1e-12);
    assert!((title_meeting.score - config.title_beta * 1.0).abs() < 1e-12);
}

#[test]
fn zero_weighted_channel_deweights_instead_of_deleting() {
    // A zero channel weight must rank that channel's candidates last, never
    // remove them: membership is channel presence, not score magnitude.
    let config = RankingConfig {
        w_lexical: 0.0,
        ..RankingConfig::chat()
    };
    let semantic = semantic_candidate("doc-s", "m1", "transcript", None, None, None, 0, 1);
    let lexical = lexical_candidate(
        "fts-l",
        "m2",
        "transcript",
        None,
        None,
        QueryVariantKind::Original,
        Some(LexicalMode::And),
        1,
    );
    let fused = fuse(&[semantic, lexical], &config);
    let order: Vec<&str> = fused
        .iter()
        .map(|entry| entry.evidence.evidence_id.as_str())
        .collect();
    assert_eq!(order, ["doc-s", "fts-l"]);
    assert!((fused[1].fused_score - 0.0).abs() < 1e-12);
}

#[test]
fn title_overlap_normalizes_and_dedupes() {
    let core_terms: Vec<String> = vec!["retencao".to_string(), "whatsapp".to_string()];
    let mut target = semantic_candidate("t", "mtg-x", "transcript", None, None, None, 0, 1);
    target.meeting_title = "R\u{e9}gua WhatsApp de reten\u{e7}\u{e3}o".to_string();
    target.text = "contatos por WhatsApp no fluxo de reten\u{e7}\u{e3}o".to_string();
    let rival = lexical_candidate(
        "r",
        "mtg-outro",
        "transcript",
        None,
        None,
        QueryVariantKind::Original,
        Some(LexicalMode::Or),
        1,
    );
    let title = title_overlap(&[target, rival], &core_terms);
    assert!((title["mtg-x"] - 2.0 / 2.0).abs() < 1e-12);
    assert!((title["mtg-outro"] - 0.0).abs() < 1e-12);
}

#[tokio::test]
async fn ordered_segment_ids_match_the_chunker_chronology() {
    let pool = migrated_pool().await;
    sqlx::query(
        "INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'M', '2026-08-29', '2026-08-29')",
    )
    .execute(&pool)
    .await
    .unwrap();
    for (id, audio) in [("s1", 30.0), ("s2", 10.0), ("s3", 20.0)] {
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, ?, ?, '10:00', ?)")
            .bind(id)
            .bind("m1")
            .bind(format!("{id} segmento com conteudo real da reuniao"))
            .bind(audio)
            .execute(&pool)
            .await
            .unwrap();
    }
    let order = RetrievalRepository::ordered_transcript_segment_ids(
        &pool,
        &["m1".to_string()],
        &CancellationToken::new(),
    )
    .await
    .expect("ordered segment ids");
    assert_eq!(order.segments["m1"], vec!["s2", "s3", "s1"]);
    pool.close().await;
}

#[test]
fn chunker_windows_sit_inside_the_ordered_chronology() {
    // The chunker's windows cover exactly the chronological positions the
    // repository orders: the first window starts at the ordered head and the
    // last window ends at the ordered tail.
    let source = MeetingSource {
        meeting_id: "m1".to_string(),
        title: "M".to_string(),
        folder_id: None,
        folder_name: String::new(),
        source_revision: Some(1),
        latest_summary_template_id: None,
        latest_summary_markdown: None,
        notes_markdown: None,
        transcripts: [("s1", 30.0), ("s2", 10.0), ("s3", 20.0)]
            .into_iter()
            .map(|(id, audio)| SourceTranscript {
                id: id.to_string(),
                text: format!("{id} palavras de conteudo para a janela"),
                speaker: None,
                timestamp: "10:00".to_string(),
                audio_start_time: Some(audio),
                audio_end_time: None,
            })
            .collect(),
        complete: true,
        transcript_positions: vec![0, 1, 2],
        transcript_segments_total: 3,
    };
    let documents = chunk_meeting(
        &source,
        &ChunkerConfig {
            window_tokens: 4,
            overlap_tokens: 1,
            ..ChunkerConfig::default()
        },
        &Words,
    );
    let windows: Vec<&crate::retrieval::chunking::SemanticDocument> = documents
        .iter()
        .filter(|doc| doc.source_kind == "transcript")
        .collect();
    assert!(windows.len() >= 2);
    let first = windows[0].transcript.as_ref().expect("window range");
    assert_eq!(first.start_segment_id, "s2");
    let last = windows
        .last()
        .unwrap()
        .transcript
        .as_ref()
        .expect("window range");
    assert_eq!(last.end_segment_id, "s1");
}
