//! Deterministic semantic document chunking (Sprint 2A Task 2.3).
//!
//! Turns the authoritative [`MeetingSource`] read by the retrieval repository
//! into stable meeting-profile, transcript-window, summary-section, and
//! notes-section documents under the Sprint 1 approved contract: 384-token
//! windows with 64-token overlap, the pinned multilingual-e5 tokenizer, and
//! the manifest `documentPrefix` as the only text normalization. Text sent to
//! the embedding model is `DOCUMENT_PREFIX + content`; the content hash covers
//! exactly that string, and `document_id` is a SHA-256 over the stable
//! model/chunker/source/ordinal/hash tuple, so identical inputs are
//! byte-identical across runs and processes.
//!
//! Evidence fields (`source_kind`, transcript range IDs, summary template ID,
//! heading, ordinal) survive on every document so hydration can re-read the
//! authoritative rows. Folder metadata is deliberately absent: the approved
//! contract keeps it outside embedded content. This module never logs and is
//! infallible; malformed or empty sources simply produce fewer documents.

use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

use crate::database::repositories::retrieval::{MeetingSource, SourceTranscript};

/// Embedding model identity pinned by the packaged bundle manifest; a test
/// keeps these constants and the manifest from drifting apart.
pub const APPROVED_MODEL_ID: &str = "intfloat/multilingual-e5-base";
/// `chunkerVersion` in the same manifest.
pub const APPROVED_CHUNKER_VERSION: u32 = 1;
/// Sprint 1 approved chunk profile: "384 tokens with 64-token overlap".
pub const WINDOW_TOKENS: usize = 384;
pub const OVERLAP_TOKENS: usize = 64;
/// Manifest `documentPrefix`: applied by the embedding runtime before
/// inference and covered by every content hash.
pub const DOCUMENT_PREFIX: &str = "passage: ";

pub const SOURCE_KIND_PROFILE: &str = "meeting_profile";
pub const SOURCE_KIND_TRANSCRIPT: &str = "transcript";
pub const SOURCE_KIND_SUMMARY: &str = "summary";
pub const SOURCE_KIND_NOTES: &str = "notes";

/// Chunking identity and window policy. [`ChunkerConfig::default`] is the
/// approved Sprint 1 profile; custom values require `overlap_tokens <
/// window_tokens` and both nonzero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkerConfig {
    pub model_id: String,
    pub chunker_version: u32,
    pub window_tokens: usize,
    pub overlap_tokens: usize,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            model_id: APPROVED_MODEL_ID.to_string(),
            chunker_version: APPROVED_CHUNKER_VERSION,
            window_tokens: WINDOW_TOKENS,
            overlap_tokens: OVERLAP_TOKENS,
        }
    }
}

/// Token-counting policy for window arithmetic. Production callers pass the
/// pinned packaged tokenizer wrapped in [`PackagedTokenizer`]; tests may use a
/// deterministic adapter. Counts never come from bytes or characters.
pub trait TokenizerPolicy {
    /// Number of model tokens for `text`, excluding the document prefix and
    /// special tokens.
    fn count_tokens(&self, text: &str) -> usize;
}

/// Counts content tokens with one of the bundled tokenizer artifacts (the same
/// files the embedding runtime loads), so window budgets are expressed in true
/// model tokens.
pub struct PackagedTokenizer<'a> {
    tokenizer: &'a Tokenizer,
}

impl<'a> PackagedTokenizer<'a> {
    pub fn new(tokenizer: &'a Tokenizer) -> Self {
        Self { tokenizer }
    }
}

impl TokenizerPolicy for PackagedTokenizer<'_> {
    fn count_tokens(&self, text: &str) -> usize {
        self.tokenizer
            .encode(text, false)
            .map(|encoded| encoded.get_ids().len())
            // Counting failure must over-split, never over-stuff: an
            // "unbounded" atom is emitted alone instead of merged into a
            // window that could exceed the model limit.
            .unwrap_or(usize::MAX)
    }
}

/// Authoritative-row pointers for rehydrating a transcript window: the
/// window's first/last segment IDs address the exact `transcripts` rows, and
/// their speaker/timestamp bounds travel with the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptProvenance {
    pub start_segment_id: String,
    pub end_segment_id: String,
    pub start_speaker: Option<String>,
    pub end_speaker: Option<String>,
    pub start_timestamp: String,
    pub end_timestamp: String,
}

/// One derived semantic document ready for embedding and staging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticDocument {
    pub document_id: String,
    pub source_kind: &'static str,
    /// 0-based position within this document's kind for the meeting, in
    /// emission order.
    pub ordinal: usize,
    /// Exact embedded text (the runtime prepends [`DOCUMENT_PREFIX`]).
    pub content: String,
    /// SHA-256 over `DOCUMENT_PREFIX + content`.
    pub content_hash: Vec<u8>,
    pub source_template_id: Option<String>,
    /// Markdown heading text for summary/notes sections.
    pub heading: Option<String>,
    pub transcript: Option<TranscriptProvenance>,
}

struct Atom {
    text: String,
    tokens: usize,
}

/// Chunks one meeting's authoritative content into every semantic document.
/// Output order is stable: profile, transcript windows in chronology order,
/// summary sections, then notes sections; ordinals restart per kind.
pub fn chunk_meeting(
    source: &MeetingSource,
    config: &ChunkerConfig,
    tokenizer: &impl TokenizerPolicy,
) -> Vec<SemanticDocument> {
    let mut documents = Vec::new();
    documents.extend(profile_document(source, config, tokenizer));
    documents.extend(transcript_documents(source, config, tokenizer));
    documents.extend(markdown_documents(
        source.meeting_id.as_str(),
        SOURCE_KIND_SUMMARY,
        source.latest_summary_template_id.as_deref(),
        source.latest_summary_markdown.as_deref(),
        config,
        tokenizer,
    ));
    documents.extend(markdown_documents(
        source.meeting_id.as_str(),
        SOURCE_KIND_NOTES,
        None,
        source.notes_markdown.as_deref(),
        config,
        tokenizer,
    ));
    documents
}

// -- Meeting profile ------------------------------------------------------

/// Builds the bounded meeting-profile document from the title, the latest
/// non-empty summary (already resolved by the repository with the saved-meeting
/// Chat policy), and current notes.
///
/// ponytail: the profile is truncated to its first window when the labeled
/// blocks exceed the token budget — profiles support meeting selection, not
/// evidence; upgrade path is bounded multi-window profiles if recall suffers.
fn profile_document(
    source: &MeetingSource,
    config: &ChunkerConfig,
    tokenizer: &impl TokenizerPolicy,
) -> Option<SemanticDocument> {
    let mut blocks: Vec<String> = Vec::new();
    if !source.title.trim().is_empty() {
        blocks.push(format!("Title: {}", source.title));
    }
    let summary = non_blank(source.latest_summary_markdown.as_deref());
    if let Some(summary) = summary {
        blocks.push(format!("Summary:\n{}", summary.trim_end()));
    }
    let notes = non_blank(source.notes_markdown.as_deref());
    if let Some(notes) = notes {
        blocks.push(format!("Notes:\n{}", notes.trim_end()));
    }
    if blocks.is_empty() {
        return None;
    }

    let content = blocks.join("\n\n");
    let content = if tokenizer.count_tokens(&content) <= config.window_tokens {
        content
    } else {
        let atoms = build_atoms(
            blocks.iter().map(String::as_str),
            config.window_tokens,
            tokenizer,
        );
        let tokens: Vec<usize> = atoms.iter().map(|atom| atom.tokens).collect();
        let (start, end) = window_ranges(&tokens, config.window_tokens, config.overlap_tokens)
            .into_iter()
            .next()?;
        atoms[start..end]
            .iter()
            .map(|atom| atom.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some(finish_document(
        config,
        source.meeting_id.as_str(),
        SOURCE_KIND_PROFILE,
        "",
        0,
        content,
        if summary.is_some() {
            source.latest_summary_template_id.clone()
        } else {
            None
        },
        None,
        None,
    ))
}

// -- Transcript windows ---------------------------------------------------

/// Chronology matching the authoritative readers (and the saved-meeting Chat
/// path): non-null `audio_start_time` first ascending, nulls last, then
/// timestamp, then stable ID.
fn ordered_transcripts(transcripts: &[SourceTranscript]) -> Vec<&SourceTranscript> {
    let mut ordered: Vec<&SourceTranscript> = transcripts.iter().collect();
    ordered.sort_by(|a, b| {
        a.audio_start_time
            .is_none()
            .cmp(&b.audio_start_time.is_none())
            .then_with(|| match (a.audio_start_time, b.audio_start_time) {
                (Some(left), Some(right)) => left.total_cmp(&right),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.timestamp.cmp(&b.timestamp))
            .then_with(|| a.id.cmp(&b.id))
    });
    ordered
}

fn transcript_documents(
    source: &MeetingSource,
    config: &ChunkerConfig,
    tokenizer: &impl TokenizerPolicy,
) -> Vec<SemanticDocument> {
    struct TranscriptAtom<'a> {
        segment: &'a SourceTranscript,
        atom: Atom,
    }

    let live: Vec<&SourceTranscript> = ordered_transcripts(&source.transcripts)
        .into_iter()
        .filter(|segment| !segment.text.trim().is_empty())
        .collect();
    // Whole segments are the preferred atoms; only a segment larger than the
    // window budget is split, at whitespace boundaries, so windows align to
    // segment edges whenever possible.
    let atoms: Vec<TranscriptAtom> = live
        .iter()
        .flat_map(|segment| {
            split_oversized(&segment.text, config.window_tokens, tokenizer)
                .into_iter()
                .map(|text| {
                    let tokens = tokenizer.count_tokens(&text);
                    TranscriptAtom {
                        segment,
                        atom: Atom { text, tokens },
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let tokens: Vec<usize> = atoms.iter().map(|entry| entry.atom.tokens).collect();
    window_ranges(&tokens, config.window_tokens, config.overlap_tokens)
        .into_iter()
        .enumerate()
        .map(|(ordinal, (start, end))| {
            let first = atoms[start].segment;
            let last = atoms[end - 1].segment;
            let content = atoms[start..end]
                .iter()
                .map(|entry| entry.atom.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            finish_document(
                config,
                source.meeting_id.as_str(),
                SOURCE_KIND_TRANSCRIPT,
                &format!("{}..{}", first.id, last.id),
                ordinal,
                content,
                None,
                None,
                Some(TranscriptProvenance {
                    start_segment_id: first.id.clone(),
                    end_segment_id: last.id.clone(),
                    start_speaker: first.speaker.clone(),
                    end_speaker: last.speaker.clone(),
                    start_timestamp: first.timestamp.clone(),
                    end_timestamp: last.timestamp.clone(),
                }),
            )
        })
        .collect()
}

// -- Markdown sections ----------------------------------------------------

struct Section {
    /// Verbatim `# ...` line, when the section has a heading.
    heading_line: Option<String>,
    heading_text: Option<String>,
    /// Verbatim body lines, including blank separators.
    body_lines: Vec<String>,
}

/// Splits Markdown into heading sections in document order. ATX headings
/// (`#`..`######` followed by whitespace) open a section; text before the
/// first heading becomes a headingless preamble section.
fn split_markdown_sections(markdown: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current = Section {
        heading_line: None,
        heading_text: None,
        body_lines: Vec::new(),
    };
    for line in markdown.lines() {
        if let Some(heading_text) = atx_heading(line) {
            sections.push(current);
            current = Section {
                heading_line: Some(line.to_string()),
                heading_text: (!heading_text.is_empty()).then(|| heading_text.to_string()),
                body_lines: Vec::new(),
            };
        } else {
            current.body_lines.push(line.to_string());
        }
    }
    sections.push(current);
    sections
}

fn atx_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|&byte| byte == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if rest.is_empty() {
        return Some("");
    }
    rest.starts_with([' ', '\t']).then(|| rest.trim())
}

/// Emits one or more token-windowed section documents per non-empty Markdown
/// section. The heading line rides as the first atom of its section, so the
/// first window carries it and every document keeps the heading metadata.
fn markdown_documents(
    meeting_id: &str,
    kind: &'static str,
    template_id: Option<&str>,
    markdown: Option<&str>,
    config: &ChunkerConfig,
    tokenizer: &impl TokenizerPolicy,
) -> Vec<SemanticDocument> {
    let mut documents = Vec::new();
    let Some(markdown) = non_blank(markdown) else {
        return documents;
    };
    let mut ordinal = 0usize;
    for section in split_markdown_sections(markdown) {
        if section.body_lines.iter().all(|line| line.trim().is_empty()) {
            // Empty sections are omitted, heading included.
            continue;
        }
        let mut lines: Vec<&str> = Vec::with_capacity(section.body_lines.len() + 1);
        if let Some(heading_line) = &section.heading_line {
            lines.push(heading_line.as_str());
        }
        lines.extend(section.body_lines.iter().map(String::as_str));
        let atoms = build_atoms(lines.into_iter(), config.window_tokens, tokenizer);
        let tokens: Vec<usize> = atoms.iter().map(|atom| atom.tokens).collect();
        for (start, end) in window_ranges(&tokens, config.window_tokens, config.overlap_tokens) {
            let content = atoms[start..end]
                .iter()
                .map(|atom| atom.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            documents.push(finish_document(
                config,
                meeting_id,
                kind,
                "",
                ordinal,
                content,
                template_id.map(str::to_string),
                section.heading_text.clone(),
                None,
            ));
            ordinal += 1;
        }
    }
    documents
}

// -- Window mechanics -----------------------------------------------------

/// Greedy contiguous windows over atom token counts: each window holds at most
/// `window_tokens`, and the next window re-includes trailing atoms worth up to
/// `overlap_tokens`. Progress is unconditional: a lone atom larger than the
/// window forms its own window rather than being dropped or looping forever.
fn window_ranges(tokens: &[usize], window: usize, overlap: usize) -> Vec<(usize, usize)> {
    debug_assert!(window > 0 && overlap < window);
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < tokens.len() {
        let mut end = start;
        let mut used = 0;
        while end < tokens.len() && used + tokens[end] <= window {
            used += tokens[end];
            end += 1;
        }
        if end == start {
            end = start + 1;
        }
        ranges.push((start, end));
        if end == tokens.len() {
            break;
        }
        let mut next_start = end - 1;
        let mut carried = tokens[next_start];
        while next_start > start && carried + tokens[next_start - 1] <= overlap {
            next_start -= 1;
            carried += tokens[next_start];
        }
        start = next_start.max(start + 1);
    }
    ranges
}

/// One atom per input chunk, splitting any chunk above the window budget at
/// whitespace boundaries. Chunk text is preserved verbatim inside fragments.
fn build_atoms<'a>(
    chunks: impl Iterator<Item = &'a str>,
    window: usize,
    tokenizer: &impl TokenizerPolicy,
) -> Vec<Atom> {
    chunks
        .flat_map(|chunk| {
            split_oversized(chunk, window, tokenizer)
                .into_iter()
                .map(|text| {
                    let tokens = tokenizer.count_tokens(&text);
                    Atom { text, tokens }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Splits text into fragments of at most `budget` tokens, cutting only between
/// whitespace-delimited words so every original character stays intact. Chunks
/// already within the budget come back untouched; a single word above the
/// budget is emitted whole (and alone) rather than dropped or cut mid-character.
///
/// ponytail: recounting each candidate fragment is quadratic in fragment
/// count; utterances are sentence-scale so this is invisible today. Upgrade
/// path: cut at tokenizer offsets (`Encoding::offsets`) instead.
fn split_oversized(text: &str, budget: usize, tokenizer: &impl TokenizerPolicy) -> Vec<String> {
    if tokenizer.count_tokens(text) <= budget {
        return vec![text.to_string()];
    }
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut word_start: Option<usize> = None;
    for (index, character) in text.char_indices() {
        if character.is_whitespace() {
            if let Some(start) = word_start.take() {
                words.push((start, index));
            }
        } else if word_start.is_none() {
            word_start = Some(index);
        }
    }
    if let Some(start) = word_start {
        words.push((start, text.len()));
    }
    if words.is_empty() {
        return Vec::new();
    }

    let mut fragments = Vec::new();
    // Fragment zero starts at the text start so leading whitespace survives;
    // later fragments begin at their first word's offset, and every cut lands
    // on a word boundary, so the fragments tile [0, len) verbatim.
    let mut fragment_start = 0;
    let mut cursor = 0;
    while cursor < words.len() {
        let mut end = cursor;
        while end < words.len()
            && tokenizer.count_tokens(&text[fragment_start..words[end].1]) <= budget
        {
            end += 1;
        }
        if end == cursor {
            // Single word above the budget: emit it alone to keep progress
            // and full content coverage.
            end = cursor + 1;
        }
        // Cut at the next word's start (or the text end); any inter-word
        // whitespace stays with the fragment, keeping characters verbatim.
        let fragment_end = words.get(end).map_or(text.len(), |&(start, _)| start);
        fragments.push(text[fragment_start..fragment_end].to_string());
        cursor = end;
        if cursor < words.len() {
            fragment_start = words[cursor].0;
        }
    }
    fragments
}

// -- Identity -------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn finish_document(
    config: &ChunkerConfig,
    meeting_id: &str,
    kind: &'static str,
    range_key: &str,
    ordinal: usize,
    content: String,
    source_template_id: Option<String>,
    heading: Option<String>,
    transcript: Option<TranscriptProvenance>,
) -> SemanticDocument {
    // The hash covers the exact normalized text handed to the embedding model.
    let content_hash = Sha256::digest(format!("{DOCUMENT_PREFIX}{content}")).to_vec();
    let hash_hex = hex_encode(&content_hash);
    let mut hasher = Sha256::new();
    for component in [
        config.model_id.as_bytes(),
        config.chunker_version.to_string().as_bytes(),
        meeting_id.as_bytes(),
        kind.as_bytes(),
        range_key.as_bytes(),
        ordinal.to_string().as_bytes(),
        hash_hex.as_bytes(),
    ] {
        hasher.update(component);
        hasher.update([0]);
    }
    SemanticDocument {
        document_id: hex_encode(&hasher.finalize()),
        source_kind: kind,
        ordinal,
        content,
        content_hash,
        source_template_id,
        heading,
        transcript,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// Deterministic stand-in for the pinned XLM-R unigram tokenizer: each
    /// whitespace-delimited word costs one token plus one per punctuation
    /// character. It approximates subword counting without ever deriving
    /// counts from bytes or raw character totals.
    #[derive(Default)]
    struct HeuristicTokens;

    impl TokenizerPolicy for HeuristicTokens {
        fn count_tokens(&self, text: &str) -> usize {
            text.split_whitespace()
                .map(|word| 1 + word.chars().filter(|c| !c.is_alphanumeric()).count())
                .sum()
        }
    }

    const TOKENS: HeuristicTokens = HeuristicTokens;

    fn transcript(
        id: &str,
        text: &str,
        speaker: Option<&str>,
        audio: Option<f64>,
    ) -> SourceTranscript {
        SourceTranscript {
            id: id.to_string(),
            text: text.to_string(),
            speaker: speaker.map(str::to_string),
            timestamp: "10:00".to_string(),
            audio_start_time: audio,
            audio_end_time: None,
        }
    }

    fn meeting(title: &str, transcripts: Vec<SourceTranscript>) -> MeetingSource {
        MeetingSource {
            meeting_id: "m-1".to_string(),
            title: title.to_string(),
            folder_id: None,
            folder_name: String::new(),
            source_revision: Some(7),
            latest_summary_template_id: None,
            latest_summary_markdown: None,
            notes_markdown: None,
            transcripts,
            transcript_positions: Vec::new(),
            transcript_segments_total: 0,
            complete: true,
        }
    }

    /// Repeats `prefix<N>` words so each occurrence is a distinct deterministic
    /// token, producing prose of roughly `words` heuristic tokens.
    fn filler(prefix: &str, words: usize) -> String {
        (0..words)
            .map(|index| format!("{prefix}{index}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn kinds(documents: &[SemanticDocument], kind: &'static str) -> Vec<SemanticDocument> {
        documents
            .iter()
            .filter(|document| document.source_kind == kind)
            .cloned()
            .collect()
    }

    #[test]
    fn packaged_manifest_pins_the_chunker_constants() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ManifestProbe {
            chunker_version: u32,
            embedding_model: EmbeddingProbe,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct EmbeddingProbe {
            model_id: String,
            document_prefix: String,
            max_sequence_length: u32,
        }
        let manifest: ManifestProbe = serde_json::from_str(include_str!(
            "../../resources/retrieval/model-bundle.manifest.json"
        ))
        .expect("packaged manifest must parse");
        assert_eq!(APPROVED_MODEL_ID, manifest.embedding_model.model_id);
        assert_eq!(APPROVED_CHUNKER_VERSION, manifest.chunker_version);
        assert_eq!(DOCUMENT_PREFIX, manifest.embedding_model.document_prefix);
        // The window profile must leave room for the prefix and special tokens
        // inside the manifest max sequence length.
        assert_eq!(WINDOW_TOKENS, 384);
        assert_eq!(OVERLAP_TOKENS, 64);
        assert!(WINDOW_TOKENS + 8 <= manifest.embedding_model.max_sequence_length as usize);
    }

    #[test]
    fn chunking_is_byte_identical_and_golden_ids_hold() {
        let source = fixture_source();

        let config = ChunkerConfig::default();
        let first = chunk_meeting(&source, &config, &TOKENS);
        let second = chunk_meeting(&source, &config, &TOKENS);
        assert_eq!(first, second, "identical inputs must be byte-identical");

        for document in &first {
            assert_eq!(document.document_id.len(), 64);
            assert!(
                document
                    .document_id
                    .chars()
                    .all(|character| character.is_ascii_hexdigit()),
                "{}",
                document.document_id
            );
            assert_eq!(document.content_hash.len(), 32);
        }

        // Golden identities for this fixture: any drift in the hashing inputs,
        // prefix, or window policy shows up here. The three short utterances
        // pack into one segment-aligned window.
        let transcript_docs = kinds(&first, SOURCE_KIND_TRANSCRIPT);
        assert_eq!(transcript_docs.len(), 1);
        let range = transcript_docs[0].transcript.as_ref().unwrap();
        assert_eq!(range.start_segment_id, "t1");
        assert_eq!(range.end_segment_id, "t3");
        assert_eq!(
            transcript_docs[0].document_id,
            "5b4e0913a69fc33d14cfd167e6543b3df84086df2626e5e6de21e4a2d06f15b2"
        );
        assert_eq!(range.start_speaker.as_deref(), Some("mic"));
        assert_eq!(range.end_speaker.as_deref(), Some("mic"));
        let summaries = kinds(&first, SOURCE_KIND_SUMMARY);
        assert_eq!(summaries.len(), 2);
        assert_eq!(
            summaries[0].document_id,
            "8b19d45933aa39f6745ec2600ee6a41f134850120986fc2dd06bf373508ca96c"
        );
    }

    fn fixture_source() -> MeetingSource {
        let mut source = meeting(
            "Revisão Trimestral — Comunicação",
            vec![
                transcript(
                    "t3",
                    "Validamos o fluxo de retenção.",
                    Some("mic"),
                    Some(12.5),
                ),
                transcript("t1", "Abertura da reunião de hoje.", Some("mic"), Some(0.0)),
                transcript("t2", "Apresentação do relatório mensal 📊", None, Some(6.0)),
            ],
        );
        source.latest_summary_template_id = Some("executive-summary".to_string());
        source.latest_summary_markdown = Some(
            "## Decisões\n- Manter o cronograma de comunicação\n\n## Riscos\n- Atraso no envio"
                .to_string(),
        );
        source.notes_markdown = Some("Enviar resumo por e-mail.".to_string());
        source
    }

    #[test]
    fn one_edit_only_changes_windows_containing_that_segment() {
        let base = vec![
            transcript("s1", &filler("a", 180), Some("mic"), Some(1.0)),
            transcript("s2", &filler("b", 180), Some("mic"), Some(2.0)),
            transcript("s3", "delta tres quatro cinco", Some("system"), Some(3.0)),
            transcript("s4", &filler("d", 180), Some("mic"), Some(4.0)),
        ];
        let config = ChunkerConfig::default();
        let before = chunk_meeting(&meeting("M", base.clone()), &config, &TOKENS);

        let mut edited = base;
        edited[2].text = "delta tres cinco cinco".to_string();
        let after = chunk_meeting(&meeting("M", edited), &config, &TOKENS);

        assert_eq!(
            before.len(),
            after.len(),
            "equal-size edits keep the window layout"
        );
        for (old, new) in before.iter().zip(after.iter()) {
            let touches_edit = old
                .transcript
                .as_ref()
                .map(|range| {
                    let start = range.start_segment_id.as_str();
                    let end = range.end_segment_id.as_str();
                    start <= "s3" && "s3" <= end
                })
                .unwrap_or(false);
            if touches_edit {
                assert_ne!(old.content_hash, new.content_hash);
            } else {
                assert_eq!(old, new, "untouched windows must not move or change");
            }
        }
    }

    #[test]
    fn transcript_windows_respect_limits_overlap_and_drop_nothing() {
        let segments: Vec<SourceTranscript> = (0..12)
            .map(|index| {
                transcript(
                    &format!("t{index:02}"),
                    &filler(&format!("w{index:02}_"), 90),
                    Some("mic"),
                    Some(index as f64),
                )
            })
            .collect();
        let config = ChunkerConfig::default();
        let documents = kinds(
            &chunk_meeting(&meeting("M", segments.clone()), &config, &TOKENS),
            SOURCE_KIND_TRANSCRIPT,
        );
        assert!(documents.len() >= 2, "expected multiple windows");

        let segment_ids_of = |document: &SemanticDocument| -> Vec<String> {
            document
                .content
                .lines()
                .filter_map(|line| {
                    segments
                        .iter()
                        .find(|segment| segment.text == line)
                        .map(|segment| segment.id.clone())
                })
                .collect()
        };

        let covered: Vec<Vec<String>> = documents.iter().map(segment_ids_of).collect();
        for window in &covered {
            assert!(!window.is_empty(), "windows never drop every segment");
        }
        for pair in covered.windows(2) {
            let (previous, next) = (&pair[0], &pair[1]);
            assert!(
                previous.contains(&next[0]),
                "adjacent windows must overlap instead of dropping segments"
            );
        }
        for segment in &segments {
            assert!(
                covered.iter().flatten().any(|id| id == &segment.id),
                "segment {} disappeared between windows",
                segment.id
            );
        }
        for document in &documents {
            assert!(
                TOKENS.count_tokens(&document.content) <= WINDOW_TOKENS,
                "window exceeded the token budget"
            );
            // Provenance bounds match the segments actually inside the window.
            let range = document.transcript.as_ref().unwrap();
            assert_eq!(range.start_segment_id, segment_ids_of(document)[0]);
            assert_eq!(
                range.end_segment_id,
                segment_ids_of(document).last().unwrap().clone()
            );
        }
    }

    #[test]
    fn oversized_segment_splits_at_word_boundaries_without_loss() {
        let long_text = filler("u", 1500);
        let segments = vec![
            transcript("before", "Contexto anterior curto.", Some("mic"), Some(1.0)),
            transcript("huge", &long_text, Some("system"), Some(2.0)),
            transcript("after", "Encerramento da pauta.", None, Some(3.0)),
        ];
        let config = ChunkerConfig::default();
        let documents = kinds(
            &chunk_meeting(&meeting("M", segments), &config, &TOKENS),
            SOURCE_KIND_TRANSCRIPT,
        );
        assert!(documents.len() >= 4, "the long utterance must span windows");
        for document in &documents {
            assert!(TOKENS.count_tokens(&document.content) <= WINDOW_TOKENS);
            assert!(document.transcript.is_some());
        }
        let combined: String = documents
            .iter()
            .map(|document| document.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        for word in long_text.split_whitespace().step_by(97) {
            assert!(
                combined.contains(word),
                "lost oversized-segment word {word}"
            );
        }
        assert!(combined.contains("Contexto anterior curto."));
        assert!(combined.contains("Encerramento da pauta."));
        // The oversized segment keeps one continuous provenance identity.
        assert!(documents.iter().any(|document| document
            .transcript
            .as_ref()
            .unwrap()
            .start_segment_id
            == "huge"));
    }

    #[test]
    fn oversized_split_preserves_leading_and_trailing_whitespace() {
        // Regression: the first fragment used to start at the first word,
        // silently dropping leading whitespace. The approved contract permits
        // no text normalization, so fragments must tile the input verbatim.
        let text = format!(
            "\n\t  {}  middle   {}\n\t",
            filler("u", 900),
            filler("v", 600)
        );
        let config = ChunkerConfig::default();
        let fragments = split_oversized(&text, config.window_tokens, &TOKENS);
        assert!(fragments.len() >= 3, "expected several fragments");
        assert_eq!(
            fragments.concat(),
            text,
            "fragments must reproduce the input byte-for-byte"
        );

        // Through the chunker: the first embedded window keeps the raw prefix.
        let source = meeting("M", vec![transcript("lead", &text, Some("mic"), Some(1.0))]);
        let documents = kinds(
            &chunk_meeting(&source, &config, &TOKENS),
            SOURCE_KIND_TRANSCRIPT,
        );
        assert!(documents.len() >= 2);
        assert!(
            documents[0].content.starts_with("\n\t  "),
            "leading whitespace must survive into the embedded content"
        );
        assert!(
            documents.last().unwrap().content.ends_with('\t'),
            "trailing whitespace must survive into the embedded content"
        );
    }

    #[test]
    fn summary_sections_keep_headings_templates_and_ordinals() {
        let markdown = "linha introdutória\n\n## Decisões\n- manter cronograma çã\n\n## Vazia\n\n## Decisões\n- revisar prazos\n\n### Ações\n- enviar e-mail".to_string();
        let mut source = meeting("M", vec![transcript("t1", "texto", None, None)]);
        source.latest_summary_template_id = Some("tpl-1".to_string());
        source.latest_summary_markdown = Some(markdown);
        source.notes_markdown = Some("# Notas\n- lembrar da ata".to_string());

        let config = ChunkerConfig::default();
        let documents = chunk_meeting(&source, &config, &TOKENS);

        let summaries = kinds(&documents, SOURCE_KIND_SUMMARY);
        let headings: Vec<Option<&str>> = summaries
            .iter()
            .map(|document| document.heading.as_deref())
            .collect();
        assert_eq!(
            headings,
            vec![None, Some("Decisões"), Some("Decisões"), Some("Ações")],
            "empty section omitted; repeated headings preserved"
        );
        assert!(summaries
            .iter()
            .all(|document| document.source_template_id.as_deref() == Some("tpl-1")));
        assert_eq!(
            summaries
                .iter()
                .map(|document| document.ordinal)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert!(summaries[1].content.starts_with("## Decisões"));
        assert!(!summaries
            .iter()
            .any(|document| document.content.contains("Vazia")));

        let notes = kinds(&documents, SOURCE_KIND_NOTES);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].heading.as_deref(), Some("Notas"));
        assert!(notes[0].source_template_id.is_none());
        assert!(notes[0].content.contains("lembrar da ata"));

        // Repeated headings must not collide on identity: ordinals differ.
        assert_ne!(summaries[1].document_id, summaries[2].document_id);
    }

    #[test]
    fn long_sections_split_within_budget_with_sequential_ordinals() {
        let body: String = (0..30)
            .map(|index| {
                format!("- ponto {index} com conteúdo de apoio detalhado para a revisão da pauta")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let markdown = format!("## Pauta longa\n{body}");
        let mut source = meeting("M", Vec::new());
        source.latest_summary_template_id = Some("tpl-2".to_string());
        source.latest_summary_markdown = Some(markdown);

        let config = ChunkerConfig::default();
        let summaries = kinds(
            &chunk_meeting(&source, &config, &TOKENS),
            SOURCE_KIND_SUMMARY,
        );
        assert!(summaries.len() >= 2);
        for (ordinal, document) in summaries.iter().enumerate() {
            assert_eq!(document.ordinal, ordinal);
            assert_eq!(document.heading.as_deref(), Some("Pauta longa"));
            assert!(TOKENS.count_tokens(&document.content) <= WINDOW_TOKENS);
        }
        assert!(summaries[0].content.starts_with("## Pauta longa"));
        // Overlap: consecutive windows share at least one body line.
        let last_line_of =
            |document: &SemanticDocument| document.content.lines().last().unwrap().to_string();
        for pair in summaries.windows(2) {
            assert!(pair[1].content.contains(&last_line_of(&pair[0])));
        }
    }

    #[test]
    fn meeting_profile_follows_latest_summary_policy() {
        let config = ChunkerConfig::default();

        // Full profile with summary and notes.
        let mut source = meeting("Reunião de Planejamento", Vec::new());
        source.latest_summary_template_id = Some("exec".to_string());
        source.latest_summary_markdown = Some("## Resumo\nDiscutimos prazos.".to_string());
        source.notes_markdown = Some("- comprar café".to_string());
        let documents = chunk_meeting(&source, &config, &TOKENS);
        let profile = &kinds(&documents, SOURCE_KIND_PROFILE)[0];
        assert!(profile.content.contains("Title: Reunião de Planejamento"));
        assert!(profile.content.contains("Summary:"));
        assert!(profile.content.contains("Notes:"));
        assert_eq!(profile.source_template_id.as_deref(), Some("exec"));
        assert_eq!(profile.ordinal, 0);

        // Without a summary the label disappears and the template is unset.
        source.latest_summary_template_id = None;
        source.latest_summary_markdown = None;
        let documents = chunk_meeting(&source, &config, &TOKENS);
        let profile = &kinds(&documents, SOURCE_KIND_PROFILE)[0];
        assert!(!profile.content.contains("Summary:"));
        assert!(profile.content.contains("Notes:"));
        assert!(profile.source_template_id.is_none());

        // Title-only meetings still select; fully empty ones produce nothing.
        let title_only = meeting("Solo", Vec::new());
        let documents = chunk_meeting(&title_only, &config, &TOKENS);
        assert_eq!(documents.len(), 1);
        assert!(documents[0].content.contains("Title: Solo"));
        let empty = meeting("", Vec::new());
        assert!(chunk_meeting(&empty, &config, &TOKENS).is_empty());
    }

    #[test]
    fn transcript_order_matches_authoritative_chronology() {
        let segments = vec![
            transcript("z9", "sem áudio tres quatro cinco", None, None),
            transcript(
                "b2",
                "audio dois tres quatro cinco",
                Some("mic"),
                Some(20.0),
            ),
            transcript("a1", "audio um tres quatro cinco", Some("mic"), Some(10.0)),
            transcript("y8", "sem audio dois quatro cinco", None, None),
            transcript("b1", "empate no instante cinco", Some("mic"), Some(10.0)),
            transcript("c3", "audio tres quatro cinco seis", None, Some(30.0)),
        ];
        let config = ChunkerConfig {
            window_tokens: 5,
            overlap_tokens: 2,
            ..ChunkerConfig::default()
        };
        let documents = kinds(
            &chunk_meeting(&meeting("M", segments), &config, &TOKENS),
            SOURCE_KIND_TRANSCRIPT,
        );
        let order: Vec<String> = documents
            .iter()
            .map(|document| {
                document
                    .transcript
                    .as_ref()
                    .unwrap()
                    .start_segment_id
                    .clone()
            })
            .collect();
        assert_eq!(
            order,
            vec!["a1", "b1", "b2", "c3", "y8", "z9"],
            "audio time asc, then timestamp, then id; null audio times last"
        );
    }

    #[test]
    fn unicode_content_survives_window_boundaries() {
        let segments = vec![
            transcript(
                "u1",
                "Comunicação é essencial para o fluxo de retenção 📈",
                Some("mic"),
                Some(1.0),
            ),
            transcript(
                "u2",
                "日本語のテキストは空白をほとんど使いません",
                None,
                Some(2.0),
            ),
            transcript(
                "u3",
                "Ação: avançar com o cronograma",
                Some("system"),
                Some(3.0),
            ),
        ];
        let config = ChunkerConfig {
            window_tokens: 12,
            overlap_tokens: 3,
            ..ChunkerConfig::default()
        };
        let documents = chunk_meeting(&meeting("M", segments.clone()), &config, &TOKENS);
        let transcript_docs = kinds(&documents, SOURCE_KIND_TRANSCRIPT);
        let combined: String = transcript_docs
            .iter()
            .map(|document| document.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for segment in &segments {
            assert!(
                combined.contains(segment.text.as_str()),
                "multibyte content lost for {}",
                segment.id
            );
        }
        // Hashes stay deterministic across repeated runs with Unicode input.
        assert_eq!(
            transcript_docs,
            kinds(
                &chunk_meeting(&meeting("M", segments), &config, &TOKENS),
                SOURCE_KIND_TRANSCRIPT
            )
        );
    }

    #[test]
    fn degenerate_inputs_produce_no_panics_and_no_ghost_documents() {
        let config = ChunkerConfig::default();

        // Malformed summary JSON arriving as extracted markdown is treated as
        // opaque prose, never parsed or logged.
        let mut broken = meeting("M", Vec::new());
        broken.latest_summary_template_id = Some("tpl".to_string());
        broken.latest_summary_markdown = Some("{\"markdown\": [unterminated".to_string());
        let documents = chunk_meeting(&broken, &config, &TOKENS);
        assert_eq!(kinds(&documents, SOURCE_KIND_SUMMARY).len(), 1);

        // Whitespace-only content yields nothing.
        let mut blank = meeting("   ", Vec::new());
        blank.notes_markdown = Some("   \n\t ".to_string());
        blank.latest_summary_markdown = Some("".to_string());
        assert!(chunk_meeting(&blank, &config, &TOKENS).is_empty());

        // Blank transcript segments are explicitly excluded as empty; the
        // title-only profile still selects the meeting.
        let blanks = vec![
            transcript("e1", "", None, None),
            transcript("e2", "  \n ", Some("mic"), Some(1.0)),
        ];
        let documents = chunk_meeting(&meeting("M", blanks), &config, &TOKENS);
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].source_kind, SOURCE_KIND_PROFILE);
        assert!(kinds(&documents, SOURCE_KIND_TRANSCRIPT).is_empty());

        // Missing timestamps and audio times do not disturb determinism.
        let mut sparse = meeting("M", vec![transcript("s1", "conteúdo solto", None, None)]);
        sparse.transcripts[0].timestamp = String::new();
        let once = chunk_meeting(&sparse, &config, &TOKENS);
        assert_eq!(once, chunk_meeting(&sparse, &config, &TOKENS));
    }

    #[test]
    fn packaged_tokenizer_keeps_real_windows_within_budget() {
        let dir = std::env::var("MEETLY_RAG_BUNDLE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("resources")
                    .join("retrieval")
                    .join("bundle")
            });
        let tokenizer_path = dir.join("tokenizers/embedding/tokenizer.json");
        if !tokenizer_path.is_file() {
            println!("SKIP packaged tokenizer windows: no staged tokenizer artifact");
            return;
        }
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .unwrap_or_else(|error| panic!("staged tokenizer failed to load: {error}"));
        let policy = PackagedTokenizer::new(&tokenizer);

        let segments_for = || {
            (0..8)
                .map(|index| {
                    transcript(
                        &format!("r{index}"),
                        &format!(
                            "Na reunião de hoje discutimos o cronograma de retenção e os próximos passos {}",
                            filler("pt", 120)
                        ),
                        Some("mic"),
                        Some(index as f64),
                    )
                })
                .collect::<Vec<_>>()
        };
        let config = ChunkerConfig::default();
        let documents = kinds(
            &chunk_meeting(&meeting("M", segments_for()), &config, &policy),
            SOURCE_KIND_TRANSCRIPT,
        );
        assert!(documents.len() >= 2);
        for document in &documents {
            assert!(
                policy.count_tokens(&document.content) <= WINDOW_TOKENS,
                "real-token window exceeded the approved profile"
            );
            assert!(
                policy.count_tokens(&format!("{DOCUMENT_PREFIX}{}", document.content)) <= 512,
                "prefixed window must fit the model sequence limit"
            );
        }
        // Deterministic identities under the production tokenizer policy.
        assert_eq!(
            documents,
            kinds(
                &chunk_meeting(&meeting("M", segments_for()), &config, &policy),
                SOURCE_KIND_TRANSCRIPT
            )
        );
    }
}
