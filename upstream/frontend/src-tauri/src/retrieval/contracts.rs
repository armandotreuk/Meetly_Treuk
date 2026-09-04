use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::hydration::{HydratedContext, HydratedSource};
use super::service::{
    EvidenceProvenance, PersistedRetrievalScope, QueryVariantKind, RankedRetrieval,
    RetrievalChannel, SemanticFallbackReason, MAX_ALLOWED_MEETING_IDS,
};

pub const MAX_HYBRID_QUERY_CHARS: usize = 2_048;
pub const MAX_HYBRID_SEARCH_RESULTS: usize = 50;
pub const MAX_HYBRID_SEARCH_MEETINGS: usize = 50;
pub const MAX_HYBRID_CONTEXT_CHARS: usize = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HybridScope {
    All {},
    Meeting {
        #[serde(rename = "meetingId")]
        meeting_id: String,
    },
    Folder {
        #[serde(rename = "folderId")]
        folder_id: String,
    },
    AllowedMeetingIds {
        #[serde(rename = "meetingIds")]
        meeting_ids: Vec<String>,
    },
}

impl HybridScope {
    pub fn into_persisted(self) -> Result<PersistedRetrievalScope, String> {
        match self {
            Self::All {} => Ok(PersistedRetrievalScope::All),
            Self::Meeting { meeting_id } => {
                Ok(PersistedRetrievalScope::Meeting(valid_id(meeting_id)?))
            }
            Self::Folder { folder_id } => Ok(PersistedRetrievalScope::Folder(valid_id(folder_id)?)),
            Self::AllowedMeetingIds { meeting_ids } => {
                if meeting_ids.len() > MAX_ALLOWED_MEETING_IDS {
                    return Err("Invalid hybrid scope".to_string());
                }
                let mut ids = Vec::with_capacity(meeting_ids.len());
                let mut seen = HashSet::new();
                for meeting_id in meeting_ids {
                    let meeting_id = valid_id(meeting_id)?;
                    if seen.insert(meeting_id.clone()) {
                        ids.push(meeting_id);
                    }
                }
                Ok(PersistedRetrievalScope::AllowedMeetingIds(ids))
            }
        }
    }
}

impl From<&PersistedRetrievalScope> for HybridScope {
    fn from(scope: &PersistedRetrievalScope) -> Self {
        match scope {
            PersistedRetrievalScope::All => Self::All {},
            PersistedRetrievalScope::Meeting(meeting_id) => Self::Meeting {
                meeting_id: meeting_id.clone(),
            },
            PersistedRetrievalScope::Folder(folder_id) => Self::Folder {
                folder_id: folder_id.clone(),
            },
            PersistedRetrievalScope::AllowedMeetingIds(meeting_ids) => Self::AllowedMeetingIds {
                meeting_ids: meeting_ids.clone(),
            },
        }
    }
}

fn valid_id(value: String) -> Result<String, String> {
    if value.is_empty()
        || value.len() > 512
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-:.".contains(character))
    {
        return Err("Invalid hybrid scope".to_string());
    }
    Ok(value)
}

pub fn validate_hybrid_query(query: &str) -> Result<(), String> {
    if query.trim().is_empty()
        || query.chars().count() > MAX_HYBRID_QUERY_CHARS
        || query.chars().any(char::is_control)
    {
        return Err("Invalid hybrid query".to_string());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HybridRetrievalStatus {
    Hybrid,
    ForcedLexical,
    LexicalFallback,
}

impl HybridRetrievalStatus {
    fn from_fallback(fallback: Option<&SemanticFallbackReason>) -> Self {
        match fallback {
            Some(SemanticFallbackReason::ForcedLexical) => Self::ForcedLexical,
            Some(_) => Self::LexicalFallback,
            None => Self::Hybrid,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HybridProvenance {
    pub evidence_id: String,
    pub channel: String,
    pub variant: String,
    pub match_mode: Option<String>,
    pub channel_rank: usize,
    pub query_slot: u8,
}

impl HybridProvenance {
    fn from_evidence(provenance: &EvidenceProvenance, evidence_id: &str) -> Self {
        Self {
            evidence_id: evidence_id.to_string(),
            channel: match provenance.channel {
                RetrievalChannel::Lexical => "lexical",
                RetrievalChannel::Title => "title",
                RetrievalChannel::Semantic => "semantic",
            }
            .to_string(),
            variant: match provenance.variant {
                QueryVariantKind::Original => "original",
                QueryVariantKind::Rewritten => "rewritten",
                QueryVariantKind::CoreTerms => "core_terms",
            }
            .to_string(),
            match_mode: provenance.mode.map(|mode| match mode {
                super::service::LexicalMode::And => "and".to_string(),
                super::service::LexicalMode::Or => "or".to_string(),
            }),
            channel_rank: provenance.rank,
            query_slot: provenance.query_slot,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HybridSource {
    pub meeting_id: String,
    pub meeting_title: String,
    pub folder_name: String,
    pub source_kind: String,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_start_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_template_id: Option<String>,
    pub evidence_ids: Vec<String>,
}

impl From<&HydratedSource> for HybridSource {
    fn from(source: &HydratedSource) -> Self {
        Self {
            meeting_id: source.meeting_id.clone(),
            meeting_title: source.meeting_title.clone(),
            folder_name: source.folder_name.clone(),
            source_kind: source.source_kind.clone(),
            snippet: source.snippet.clone(),
            source_start_id: source.source_start_id.clone(),
            source_end_id: source.source_end_id.clone(),
            source_template_id: source.source_template_id.clone(),
            evidence_ids: source.evidence_ids.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HybridMeetingResult {
    pub meeting_id: String,
    pub meeting_title: String,
    pub folder_name: String,
    pub meeting_rank: usize,
    pub retained_evidence_ids: Vec<String>,
    pub sources: Vec<HybridSource>,
    pub provenance: Vec<HybridProvenance>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HybridSearchResponse {
    pub version: &'static str,
    pub scope: HybridScope,
    pub retrieval_status: HybridRetrievalStatus,
    pub results: Vec<HybridMeetingResult>,
    pub total: usize,
}

impl HybridSearchResponse {
    pub fn from_outputs(
        ranked: &RankedRetrieval,
        hydrated: &HydratedContext,
        max_results: usize,
    ) -> Self {
        let results = ranked
            .ranking
            .meetings
            .iter()
            .filter_map(|meeting| {
                let sources = hydrated
                    .sources
                    .iter()
                    .filter(|source| source.meeting_id == meeting.meeting_id)
                    .map(HybridSource::from)
                    .collect::<Vec<_>>();
                if sources.is_empty() {
                    return None;
                }
                let retained_evidence_ids = hydrated
                    .meetings
                    .iter()
                    .find(|item| item.meeting_id == meeting.meeting_id)
                    .map(|item| item.retained_evidence_ids.clone())
                    .unwrap_or_default();
                let retained = retained_evidence_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<HashSet<_>>();
                let provenance = ranked
                    .ranking
                    .evidence
                    .iter()
                    .filter(|entry| entry.evidence.meeting_id == meeting.meeting_id)
                    .flat_map(|entry| {
                        let canonical = retained
                            .contains(entry.evidence.evidence_id.as_str())
                            .then(|| {
                                entry.evidence.provenance.iter().map(|provenance| {
                                    HybridProvenance::from_evidence(
                                        provenance,
                                        &entry.evidence.evidence_id,
                                    )
                                })
                            })
                            .into_iter()
                            .flatten();
                        let aliases = entry
                            .evidence
                            .source_aliases
                            .iter()
                            .filter(|alias| retained.contains(alias.evidence_id.as_str()))
                            .flat_map(|alias| {
                                alias.provenance.iter().map(|provenance| {
                                    HybridProvenance::from_evidence(provenance, &alias.evidence_id)
                                })
                            });
                        canonical.chain(aliases)
                    })
                    .collect();
                let first_source = sources.first()?;
                Some(HybridMeetingResult {
                    meeting_id: first_source.meeting_id.clone(),
                    meeting_title: first_source.meeting_title.clone(),
                    folder_name: first_source.folder_name.clone(),
                    meeting_rank: meeting.rank,
                    retained_evidence_ids,
                    sources,
                    provenance,
                })
            })
            .take(max_results)
            .collect::<Vec<_>>();
        Self {
            version: "v1",
            scope: HybridScope::from(&ranked.scope.scope),
            retrieval_status: HybridRetrievalStatus::from_fallback(
                ranked.semantic_fallback.as_ref(),
            ),
            total: results.len(),
            results,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HybridContextResponse {
    pub version: &'static str,
    pub scope: HybridScope,
    pub retrieval_status: HybridRetrievalStatus,
    pub context: String,
    pub retained_evidence_ids: Vec<String>,
    pub sources: Vec<HybridSource>,
}

impl HybridContextResponse {
    pub fn from_outputs(ranked: &RankedRetrieval, hydrated: &HydratedContext) -> Self {
        Self {
            version: "v1",
            scope: HybridScope::from(&ranked.scope.scope),
            retrieval_status: HybridRetrievalStatus::from_fallback(
                ranked.semantic_fallback.as_ref(),
            ),
            context: hydrated.markdown.clone(),
            retained_evidence_ids: hydrated.retained_evidence_ids.clone(),
            sources: hydrated.sources.iter().map(HybridSource::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::hydration::{HydratedMeeting, HydratedSource};
    use crate::retrieval::ranking::{
        dedupe_candidates, RankedEvidence, RankedMeeting, RankingOutcome, RerankFallback,
    };
    use crate::retrieval::service::{
        EvidenceProvenance, LexicalMode, ResolvedScope, RetrievalChannel, RetrievedEvidence,
    };
    use serde_json::json;

    #[test]
    fn hybrid_scope_is_tagged_strict_and_deduplicated() {
        let scope: HybridScope = serde_json::from_value(json!({
            "kind": "allowed_meeting_ids",
            "meetingIds": ["m1", "m1", "m2"]
        }))
        .unwrap();
        assert_eq!(
            scope.into_persisted(),
            Ok(PersistedRetrievalScope::AllowedMeetingIds(vec![
                "m1".to_string(),
                "m2".to_string()
            ]))
        );
        assert!(serde_json::from_value::<HybridScope>(json!({
            "kind": "all",
            "meetingId": "m1"
        }))
        .is_err());
        assert!(HybridScope::Meeting {
            meeting_id: "m/1".to_string()
        }
        .into_persisted()
        .is_err());
        assert!(HybridScope::AllowedMeetingIds {
            meeting_ids: vec!["m1".to_string(); MAX_ALLOWED_MEETING_IDS + 1],
        }
        .into_persisted()
        .is_err());
    }

    #[test]
    fn hybrid_query_validation_rejects_empty_control_and_oversized_input() {
        assert!(validate_hybrid_query(" \n").is_err());
        assert!(validate_hybrid_query("a\u{0000}").is_err());
        assert!(validate_hybrid_query(&"a".repeat(MAX_HYBRID_QUERY_CHARS + 1)).is_err());
        assert!(validate_hybrid_query("retention").is_ok());
    }

    #[test]
    fn public_result_limits_only_truncate_stable_ranked_outputs() {
        let count = MAX_HYBRID_SEARCH_RESULTS;
        let evidence = (0..count)
            .map(|index| RetrievedEvidence {
                evidence_id: format!("evidence-{index}"),
                meeting_id: format!("meeting-{index}"),
                meeting_title: format!("Meeting {index}"),
                source_kind: "transcript".to_string(),
                source_start_id: Some(format!("segment-{index}")),
                source_end_id: None,
                source_template_id: None,
                heading: None,
                ordinal: index as i64,
                text: format!("text {index}"),
                speaker: None,
                timestamp_label: None,
                provenance: vec![EvidenceProvenance {
                    channel: RetrievalChannel::Semantic,
                    variant: QueryVariantKind::Original,
                    mode: None,
                    rank: index + 1,
                    query_slot: 0,
                }],
                source_aliases: Vec::new(),
            })
            .collect::<Vec<_>>();
        let ranked = RankedRetrieval {
            scope: ResolvedScope {
                scope: PersistedRetrievalScope::All,
            },
            ranking: RankingOutcome {
                evidence: evidence
                    .iter()
                    .enumerate()
                    .map(|(index, evidence)| RankedEvidence {
                        evidence: evidence.clone(),
                        content_fingerprint: None,
                        fused_rank: index + 1,
                        fused_score: 1.0 / (index + 1) as f64,
                        reranker_score: None,
                    })
                    .collect(),
                meetings: (0..count)
                    .map(|index| RankedMeeting {
                        meeting_id: format!("meeting-{index}"),
                        rank: index + 1,
                        score: 1.0 / (index + 1) as f64,
                        best_fused_score: 1.0,
                        support: 1,
                        corroboration: 1,
                        title_overlap: 0.0,
                        concept_coverage: 0.0,
                    })
                    .collect(),
                reranker_used: false,
                rerank_depth: 0,
                rerank_fallback: Some(RerankFallback::Unavailable),
                core_terms: Vec::new(),
                terms: crate::retrieval::AggregationTerms::default(),
                title_overlap: std::collections::HashMap::new(),
                effective_query: "query".to_string(),
                dedupe_degraded: false,
                chronology_omitted_meetings: Vec::new(),
            },
            semantic_fallback: None,
        };
        let hydrated = HydratedContext {
            markdown: "context".to_string(),
            retained_evidence_ids: evidence
                .iter()
                .map(|item| item.evidence_id.clone())
                .collect(),
            sources: evidence
                .iter()
                .map(|item| HydratedSource {
                    meeting_id: item.meeting_id.clone(),
                    meeting_title: item.meeting_title.clone(),
                    folder_name: String::new(),
                    source_kind: item.source_kind.clone(),
                    snippet: item.text.clone(),
                    source_start_id: item.source_start_id.clone(),
                    source_end_id: item.source_end_id.clone(),
                    source_template_id: None,
                    evidence_ids: vec![item.evidence_id.clone()],
                })
                .collect(),
            meetings: evidence
                .iter()
                .enumerate()
                .map(|(index, item)| HydratedMeeting {
                    meeting_id: item.meeting_id.clone(),
                    rank: index + 1,
                    retained_evidence_ids: vec![item.evidence_id.clone()],
                    transcript_segments_included: 1,
                    transcript_segments_total: 1,
                })
                .collect(),
        };
        let all = HybridSearchResponse::from_outputs(&ranked, &hydrated, count);
        for limit in [1, 20, count] {
            let response = HybridSearchResponse::from_outputs(&ranked, &hydrated, limit);
            assert_eq!(response.results.len(), limit);
            assert_eq!(
                response
                    .results
                    .iter()
                    .map(|result| result.meeting_id.as_str())
                    .collect::<Vec<_>>(),
                all.results[..limit]
                    .iter()
                    .map(|result| result.meeting_id.as_str())
                    .collect::<Vec<_>>()
            );
        }
        let sparse = HydratedContext {
            markdown: hydrated.markdown.clone(),
            retained_evidence_ids: hydrated.retained_evidence_ids[1..].to_vec(),
            sources: hydrated.sources[1..].to_vec(),
            meetings: hydrated.meetings[1..].to_vec(),
        };
        let backfilled = HybridSearchResponse::from_outputs(&ranked, &sparse, 2);
        assert_eq!(
            backfilled
                .results
                .iter()
                .map(|result| result.meeting_id.as_str())
                .collect::<Vec<_>>(),
            ["meeting-1", "meeting-2"]
        );
        let serialized = serde_json::to_value(&all).unwrap();
        assert_eq!(
            serialized["results"][0]["provenance"][0]["evidenceId"],
            "evidence-0"
        );
        for result in &all.results {
            let retained = result.retained_evidence_ids.iter().collect::<HashSet<_>>();
            assert!(result
                .provenance
                .iter()
                .all(|item| retained.contains(&item.evidence_id)));
        }
    }

    #[test]
    fn public_provenance_keeps_canonical_and_alias_identity() {
        let canonical = RetrievedEvidence {
            evidence_id: "semantic-window".to_string(),
            meeting_id: "meeting-1".to_string(),
            meeting_title: "Meeting 1".to_string(),
            source_kind: "transcript".to_string(),
            source_start_id: Some("segment-1".to_string()),
            source_end_id: Some("segment-2".to_string()),
            source_template_id: None,
            heading: None,
            ordinal: 0,
            text: "semantic text".to_string(),
            speaker: None,
            timestamp_label: None,
            provenance: vec![EvidenceProvenance {
                channel: RetrievalChannel::Semantic,
                variant: QueryVariantKind::Original,
                mode: None,
                rank: 1,
                query_slot: 0,
            }],
            source_aliases: Vec::new(),
        };
        let lexical = RetrievedEvidence {
            evidence_id: "fts:transcript:segment-1".to_string(),
            meeting_id: "meeting-1".to_string(),
            meeting_title: "Meeting 1".to_string(),
            source_kind: "transcript".to_string(),
            source_start_id: Some("segment-1".to_string()),
            source_end_id: None,
            source_template_id: None,
            heading: None,
            ordinal: 0,
            text: "lexical text".to_string(),
            speaker: None,
            timestamp_label: None,
            provenance: vec![EvidenceProvenance {
                channel: RetrievalChannel::Lexical,
                variant: QueryVariantKind::Original,
                mode: Some(LexicalMode::Or),
                rank: 2,
                query_slot: 0,
            }],
            source_aliases: Vec::new(),
        };
        let evidence = dedupe_candidates(
            vec![canonical, lexical],
            &[(
                "meeting-1".to_string(),
                vec!["segment-1".to_string(), "segment-2".to_string()],
            )]
            .into_iter()
            .collect(),
        );
        assert_eq!(evidence.len(), 1);
        let evidence = evidence.into_iter().next().unwrap();
        let ranked = RankedRetrieval {
            scope: ResolvedScope {
                scope: PersistedRetrievalScope::All,
            },
            ranking: RankingOutcome {
                evidence: vec![RankedEvidence {
                    evidence,
                    content_fingerprint: None,
                    fused_rank: 1,
                    fused_score: 1.0,
                    reranker_score: None,
                }],
                meetings: vec![RankedMeeting {
                    meeting_id: "meeting-1".to_string(),
                    rank: 1,
                    score: 1.0,
                    best_fused_score: 1.0,
                    support: 1,
                    corroboration: 1,
                    title_overlap: 0.0,
                    concept_coverage: 1.0,
                }],
                reranker_used: false,
                rerank_depth: 0,
                rerank_fallback: Some(RerankFallback::Unavailable),
                core_terms: Vec::new(),
                terms: crate::retrieval::AggregationTerms::default(),
                title_overlap: std::collections::HashMap::new(),
                effective_query: "query".to_string(),
                dedupe_degraded: false,
                chronology_omitted_meetings: Vec::new(),
            },
            semantic_fallback: None,
        };
        let hydrated = HydratedContext {
            markdown: "context".to_string(),
            retained_evidence_ids: vec![
                "semantic-window".to_string(),
                "fts:transcript:segment-1".to_string(),
            ],
            sources: vec![HydratedSource {
                meeting_id: "meeting-1".to_string(),
                meeting_title: "Meeting 1".to_string(),
                folder_name: String::new(),
                source_kind: "transcript".to_string(),
                snippet: "authoritative text".to_string(),
                source_start_id: Some("segment-1".to_string()),
                source_end_id: Some("segment-2".to_string()),
                source_template_id: None,
                evidence_ids: vec![
                    "semantic-window".to_string(),
                    "fts:transcript:segment-1".to_string(),
                ],
            }],
            meetings: vec![HydratedMeeting {
                meeting_id: "meeting-1".to_string(),
                rank: 1,
                retained_evidence_ids: vec![
                    "semantic-window".to_string(),
                    "fts:transcript:segment-1".to_string(),
                ],
                transcript_segments_included: 2,
                transcript_segments_total: 2,
            }],
        };

        let result = HybridSearchResponse::from_outputs(&ranked, &hydrated, 1)
            .results
            .pop()
            .unwrap();
        assert_eq!(
            result
                .provenance
                .iter()
                .map(|item| (
                    item.evidence_id.as_str(),
                    item.channel.as_str(),
                    item.channel_rank
                ))
                .collect::<Vec<_>>(),
            vec![
                ("semantic-window", "semantic", 1),
                ("fts:transcript:segment-1", "lexical", 2),
            ]
        );
        assert!(!result.provenance.iter().any(|item| {
            (item.evidence_id == "semantic-window" && item.channel == "lexical")
                || (item.evidence_id == "fts:transcript:segment-1" && item.channel == "semantic")
        }));
        assert_eq!(result.sources[0].evidence_ids, result.retained_evidence_ids);
    }
}
