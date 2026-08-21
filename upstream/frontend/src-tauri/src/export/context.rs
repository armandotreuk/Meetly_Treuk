use std::collections::HashMap;

use crate::database::repositories::fts::FtsSearchResult;

pub struct MeetingContextBuild {
    pub markdown: String,
    pub retained_transcript_ids: Vec<String>,
}

pub fn build_meeting_context_markdown(
    _meeting_id: &str,
    meeting_title: &str,
    summary: Option<&str>,
    notes: Option<&str>,
    transcripts: &[FtsSearchResult],
    total_transcript_segments: usize,
    max_context_chars: usize,
) -> MeetingContextBuild {
    let mut out = format!("# Meeting Context\n\n## {}\n\n", meeting_title);
    let mandatory = [("Summary", summary), ("Notes", notes)]
        .into_iter()
        .filter_map(|(heading, content)| {
            content
                .filter(|content| !content.trim().is_empty())
                .map(|content| (heading, content))
        })
        .collect::<Vec<_>>();
    let coverage_reserve = coverage_prefix_len(total_transcript_segments);
    for (index, (heading, content)) in mandatory.iter().enumerate() {
        let remaining = mandatory.len() - index;
        let future_headers = mandatory[index + 1..]
            .iter()
            .map(|(heading, _)| {
                format!("### {}\n{}\n\n", heading, "… [truncated]")
                    .chars()
                    .count()
            })
            .sum::<usize>();
        let heading_text = format!("### {}\n", heading);
        let available = max_context_chars
            .saturating_sub(out.chars().count())
            .saturating_sub(coverage_reserve)
            .saturating_sub(future_headers)
            / remaining;
        out.push_str(&heading_text);
        let content = truncate_with_marker(
            content,
            available.saturating_sub(heading_text.chars().count() + 2),
        );
        out.push_str(&content);
        out.push_str("\n\n");
    }

    let coverage_prefix = "### Transcript coverage\n";
    let mut retained_transcript_ids = Vec::new();
    let mut transcript_text = String::new();
    for transcript in transcripts {
        let section = format_transcript(transcript);
        let coverage =
            coverage_notice(retained_transcript_ids.len() + 1, total_transcript_segments);
        let transcript_heading = if transcript_text.is_empty() {
            "### Transcript\n"
        } else {
            ""
        };
        if out.chars().count()
            + coverage_prefix.chars().count()
            + coverage.chars().count()
            + transcript_heading.chars().count()
            + transcript_text.chars().count()
            + section.chars().count()
            > max_context_chars
        {
            break;
        }
        transcript_text.push_str(&section);
        retained_transcript_ids.push(transcript.chunk_id.clone());
    }
    if retained_transcript_ids.len() < total_transcript_segments {
        let coverage = coverage_notice(retained_transcript_ids.len(), total_transcript_segments);
        out.push_str(coverage_prefix);
        out.push_str(&coverage);
        out.push_str("\n\n");
    }
    if !transcript_text.is_empty() {
        out.push_str("### Transcript\n");
        out.push_str(&transcript_text);
    }
    MeetingContextBuild {
        markdown: out,
        retained_transcript_ids,
    }
}

fn format_transcript(transcript: &FtsSearchResult) -> String {
    let mut meta = vec!["**transcript**".to_string()];
    if let Some(speaker) = &transcript.speaker {
        meta.push(format!("Speaker: {}", speaker));
    }
    if let Some(timestamp) = &transcript.timestamp_label {
        meta.push(format!("Time: {}", timestamp));
    }
    format!("> {}\n> {}\n\n", meta.join(" · "), transcript.snippet)
}

fn coverage_notice(included: usize, total: usize) -> String {
    format!(
        "Partial transcript coverage: {}/{} segments included. Disclose this limitation in your answer.",
        included, total
    )
}

fn coverage_prefix_len(total: usize) -> usize {
    if total == 0 {
        0
    } else {
        "### Transcript coverage\n".chars().count() + coverage_notice(0, total).chars().count() + 2
    }
}

fn truncate_with_marker(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let marker = "… [truncated]";
    if max_chars <= marker.chars().count() {
        return marker.chars().take(max_chars).collect();
    }
    format!(
        "{}{}",
        value
            .chars()
            .take(max_chars - marker.chars().count())
            .collect::<String>(),
        marker
    )
}

/// Group FTS results by meeting ID and build a Markdown context document
/// suitable for feeding into an LLM prompt.
///
/// The output groups chunks under meeting headers, preserving speaker and
/// timestamp metadata, so the LLM can cite specific sections.
/// Meetings appear in BM25 rank order (first result = most relevant).
pub fn build_context_markdown(results: &[FtsSearchResult]) -> String {
    build_context_markdown_with_limit(results, 100_000)
}

pub fn build_context_markdown_with_limit(
    results: &[FtsSearchResult],
    max_context_chars: usize,
) -> String {
    if results.is_empty() {
        return String::from("No relevant meeting content found.\n");
    }

    // Group by meeting_id, preserving BM25 rank order of first appearance
    let mut meeting_order: Vec<String> = Vec::new();
    let mut meetings: HashMap<String, Vec<&FtsSearchResult>> = HashMap::new();
    for r in results {
        if !meetings.contains_key(&r.meeting_id) {
            meeting_order.push(r.meeting_id.clone());
        }
        meetings.entry(r.meeting_id.clone()).or_default().push(r);
    }

    let mut out = String::with_capacity(results.len() * 256);
    out.push_str("# Meeting Context\n\n");
    out.push_str(&format!(
        "_{} matching sections from {} meeting(s)_\n\n",
        results.len(),
        meeting_order.len()
    ));

    for (i, meeting_id) in meeting_order.iter().enumerate() {
        let chunks = &meetings[meeting_id];
        let title = &chunks[0].meeting_title;
        let folder = &chunks[0].folder_name;
        out.push_str(&format!(
            "## Meeting {} — {}\n\n**ID:** `{}`\n",
            i + 1,
            title,
            meeting_id
        ));
        if !folder.is_empty() {
            out.push_str(&format!("**Folder:** {}\n", folder));
        }
        out.push('\n');

        for chunk in chunks {
            let mut meta = Vec::new();
            meta.push(format!("**{}**", chunk.chunk_type));
            if let Some(ref s) = chunk.speaker {
                meta.push(format!("Speaker: {}", s));
            }
            if let Some(ref t) = chunk.timestamp_label {
                meta.push(format!("Time: {}", t));
            }
            out.push_str(&format!("> {}\n", meta.join(" · ")));
            out.push_str(&format!("> {}\n\n", chunk.snippet));
        }
    }

    if out.chars().count() > max_context_chars {
        out = out.chars().take(max_context_chars).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::fts::FtsSearchResult;

    fn make_result(
        meeting_id: &str,
        title: &str,
        chunk_type: &str,
        snippet: &str,
        folder_name: &str,
    ) -> FtsSearchResult {
        FtsSearchResult {
            meeting_id: meeting_id.to_string(),
            meeting_title: title.to_string(),
            chunk_type: chunk_type.to_string(),
            chunk_id: format!("{}-{}", chunk_type, meeting_id),
            snippet: snippet.to_string(),
            speaker: None,
            timestamp_label: None,
            folder_id: None,
            folder_name: folder_name.to_string(),
            rank: 0.0,
        }
    }

    #[test]
    fn empty_results_returns_no_content_message() {
        let md = build_context_markdown(&[]);
        assert!(md.contains("No relevant meeting content found"));
    }

    #[test]
    fn single_meeting_single_chunk() {
        let r = make_result(
            "m1",
            "Sprint Planning",
            "transcript",
            "We decided to ship FTS5 first.",
            "Sprint 14",
        );
        let md = build_context_markdown(&[r]);
        assert!(md.contains("## Meeting 1 — Sprint Planning"));
        assert!(md.contains("`m1`"));
        assert!(md.contains("**Folder:** Sprint 14"));
        assert!(md.contains("We decided to ship FTS5 first."));
        assert!(md.contains("1 matching sections from 1 meeting"));
    }

    #[test]
    fn multiple_meetings_multiple_chunks_grouped() {
        let r1 = make_result(
            "m1",
            "Planning",
            "transcript",
            "First chunk from m1.",
            "Alpha",
        );
        let r2 = make_result("m1", "Planning", "summary", "Summary of m1.", "Alpha");
        let r3 = make_result("m2", "Retro", "transcript", "First chunk from m2.", "Beta");
        let md = build_context_markdown(&[r1, r2, r3]);
        // m1 should appear once with both chunks
        assert_eq!(md.matches("## Meeting 1 — Planning").count(), 1);
        assert!(md.contains("First chunk from m1."));
        assert!(md.contains("Summary of m1."));
        // m2 as second meeting
        assert!(md.contains("## Meeting 2 — Retro"));
        assert!(md.contains("First chunk from m2."));
        assert!(md.contains("3 matching sections from 2 meeting"));
    }

    #[test]
    fn speaker_and_timestamp_in_metadata() {
        let mut r = make_result("m1", "Meeting", "transcript", "Text.", "");
        r.speaker = Some("Alice".to_string());
        r.timestamp_label = Some("10:30".to_string());
        let md = build_context_markdown(&[r]);
        assert!(md.contains("Speaker: Alice"));
        assert!(md.contains("Time: 10:30"));
    }

    #[test]
    fn empty_folder_name_omitted() {
        let r = make_result("m1", "Meeting", "transcript", "Text.", "");
        let md = build_context_markdown(&[r]);
        assert!(!md.contains("**Folder:**"));
    }

    #[test]
    fn meeting_id_always_present() {
        let r = make_result("abc-123", "Meeting", "note", "Note text.", "General");
        let md = build_context_markdown(&[r]);
        assert!(md.contains("`abc-123`"));
    }

    #[test]
    fn rank_order_preserved_not_sorted_alphabetically() {
        // BTreeMap would sort z-project before a-project by ID.
        // We want BM25 rank order: first result = most relevant.
        let r1 = make_result("z-project", "Z Meeting", "transcript", "First ranked.", "");
        let r2 = make_result("a-project", "A Meeting", "transcript", "Second ranked.", "");
        let md = build_context_markdown(&[r1, r2]);
        let z_pos = md.find("Z Meeting").unwrap();
        let a_pos = md.find("A Meeting").unwrap();
        assert!(
            z_pos < a_pos,
            "z-project (rank 1) should appear before a-project (rank 2), but z at {} and a at {}",
            z_pos,
            a_pos
        );
    }

    #[test]
    fn context_limit_is_unicode_safe() {
        let r = make_result("m1", "Meeting", "transcript", &"🦀".repeat(100), "");
        let md = build_context_markdown_with_limit(&[r], 80);
        assert_eq!(md.chars().count(), 80);
    }

    #[test]
    fn meeting_context_tight_budget_keeps_mandatory_sections_and_coverage() {
        let transcript = make_result("m1", "Meeting", "transcript", "🦀 transcript", "");
        let built = build_meeting_context_markdown(
            "m1",
            "Meeting",
            Some(&"🦀".repeat(200)),
            Some(&"é".repeat(200)),
            &[transcript],
            10,
            350,
        );
        assert!(built.markdown.chars().count() <= 350);
        assert!(built.markdown.contains("### Summary"));
        assert!(built.markdown.contains("### Notes"));
        assert!(built.markdown.contains("… [truncated]"));
        assert!(built.markdown.contains("Partial transcript coverage: 0/10"));
        assert!(std::str::from_utf8(built.markdown.as_bytes()).is_ok());
    }
}
