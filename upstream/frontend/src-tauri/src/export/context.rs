use std::collections::HashMap;

use crate::database::repositories::fts::FtsSearchResult;

/// Group FTS results by meeting ID and build a Markdown context document
/// suitable for feeding into an LLM prompt.
///
/// The output groups chunks under meeting headers, preserving speaker and
/// timestamp metadata, so the LLM can cite specific sections.
/// Meetings appear in BM25 rank order (first result = most relevant).
pub fn build_context_markdown(results: &[FtsSearchResult]) -> String {
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
        meetings
            .entry(r.meeting_id.clone())
            .or_default()
            .push(r);
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
        let r2 = make_result(
            "m1",
            "Planning",
            "summary",
            "Summary of m1.",
            "Alpha",
        );
        let r3 = make_result(
            "m2",
            "Retro",
            "transcript",
            "First chunk from m2.",
            "Beta",
        );
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
}
