//! Tauri commands for the export module.
//!
//! Currently exposes a single command, `export_meeting_pdf`, that
//! assembles the meeting + summary + template data and returns a PDF
//! rendered by [`crate::export::pdf::export_meeting_to_pdf`].
//!
//! The command is intentionally serializable end-to-end so the
//! frontend can stream the bytes into a `dialog.save` file.

use log::{info, warn};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::DialogExt;

use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::summary::SummaryProcessesRepository;
use crate::database::repositories::templates::TemplatesRepository;
use crate::export::pdf::{export_meeting_to_pdf, MeetingExportData, SectionContent};
use crate::state::AppState;
use crate::summary::templates::{self, Template};
use tauri::State;

/// Request payload for the `export_meeting_pdf` command.
///
/// `template_id` may be either a numeric database id (stringified) for
/// user/builtin templates stored in the `templates` table, or a
/// template identifier like `daily_standup` for templates resolved
/// through the file/built-in pipeline.
#[derive(Debug, Clone, Deserialize)]
pub struct ExportPdfRequest {
    pub meeting_id: String,
    pub template_id: String,
}

/// Response payload: the rendered PDF bytes plus a suggested file name
/// (without the directory part) that the frontend can feed to
/// `dialog.save` as the default name.
#[derive(Debug, Clone, Serialize)]
pub struct ExportPdfResponse {
    /// PDF file contents.
    pub bytes: Vec<u8>,
    /// Suggested file name, e.g. `2026-06-29_weekly-planning.pdf`.
    pub suggested_filename: String,
    /// Number of pages in the produced PDF.
    pub page_count: usize,
}

/// Render a meeting summary as a PDF.
///
/// Returns the raw PDF bytes and a suggested filename. The frontend is
/// responsible for showing a save dialog and writing the bytes to
/// disk.
#[tauri::command]
pub async fn export_meeting_pdf<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, AppState>,
    request: ExportPdfRequest,
) -> Result<ExportPdfResponse, String> {
    info!(
        "export_meeting_pdf called for meeting={} template={}",
        request.meeting_id, request.template_id
    );

    let pool = state.db_manager.pool();

    // 1) Look up the meeting record (title, created_at, folder_path).
    let meeting = MeetingsRepository::get_meeting(pool, &request.meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {}", e))?
        .ok_or_else(|| format!("Meeting '{}' not found", request.meeting_id))?;

    // 2) Look up the completed summary, if any. Scoped to the requested
    //    template — only the active template's summary is exported per
    //    decision #11 (plan).
    let summary = SummaryProcessesRepository::get_summary_data(pool, &request.meeting_id, &request.template_id)
        .await
        .map_err(|e| format!("Failed to load summary: {}", e))?;

    // 3) Resolve the template (DB-stored or built-in).
    let template = resolve_template(pool, &request.template_id).await?;

    // 4) Merge template sections with the stored summary content.
    let sections = merge_sections(
        &template,
        summary.as_ref().and_then(|s| s.result.as_deref()),
    );

    // 5) Build the export payload.
    let duration = compute_duration(summary.as_ref());
    let attendees = derive_attendees(summary.as_ref());

    let created_at = meeting.created_at.clone();

    let export_data = MeetingExportData {
        meeting_id: meeting.id.clone(),
        meeting_title: if meeting.title.is_empty() {
            "Untitled meeting".to_string()
        } else {
            meeting.title.clone()
        },
        created_at,
        duration,
        attendees,
        template_name: template.name.clone(),
        sections,
    };

    // 6) Render PDF. We render in a blocking task because printpdf
    // is synchronous; offloading it to `spawn_blocking` keeps the
    // Tauri async runtime responsive.
    let data_for_render = export_data.clone();
    let (bytes, page_count) = tokio::task::spawn_blocking(move || export_meeting_to_pdf(&data_for_render))
        .await
        .map_err(|e| format!("PDF render task failed: {}", e))??;

    let suggested_filename = build_filename(&export_data);

    Ok(ExportPdfResponse {
        bytes,
        suggested_filename,
        page_count,
    })
}

/// Show a native save dialog and write the supplied PDF bytes to the
/// chosen path.
///
/// Returns the path the user selected, or `None` if the user cancelled
/// the dialog. This avoids requiring the frontend to depend on
/// `@tauri-apps/plugin-dialog`.
#[tauri::command]
pub async fn save_meeting_pdf<R: Runtime>(
    app: AppHandle<R>,
    bytes: Vec<u8>,
    suggested_filename: String,
) -> Result<Option<String>, String> {
    use std::path::PathBuf;

    let default_name = sanitize_filename(&suggested_filename);

    // The dialog plugin's blocking variant is what the rest of this
    // codebase already uses inside `async fn` commands, so we follow
    // the same pattern for consistency.
    let picked = app
        .dialog()
        .file()
        .add_filter("PDF document", &["pdf"])
        .set_file_name(&default_name)
        .set_title("Save meeting summary as PDF")
        .blocking_save_file();

    let path: PathBuf = match picked {
        Some(fp) => fp
            .into_path()
            .map_err(|e| format!("Invalid save path: {}", e))?,
        None => return Ok(None),
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create destination directory: {}", e))?;
        }
    }

    let write_path = path.clone();
    let bytes_for_write = bytes;
    tokio::task::spawn_blocking(move || std::fs::write(&write_path, &bytes_for_write))
        .await
        .map_err(|e| format!("File write task failed: {}", e))?
        .map_err(|e| format!("Failed to write PDF: {}", e))?;

    info!("Saved PDF export to {}", path.display());
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

// ---------- Helpers ----------

/// Resolve a template identifier to a `Template` struct.
///
/// Tries, in order:
/// 1. Numeric id → look in the `templates` table.
/// 2. String id → file-based lookup via `templates::get_template`.
async fn resolve_template(pool: &SqlitePool, template_id: &str) -> Result<Template, String> {
    if let Ok(id) = template_id.parse::<i64>() {
        match TemplatesRepository::get_by_id(pool, id).await {
            Ok(Some(record)) => {
                let parsed: Template = serde_json::from_str(&record.schema_json)
                    .map_err(|e| format!("Stored template has invalid schema: {}", e))?;
                return Ok(parsed);
            }
            Ok(None) => {
                // Not a DB id; fall through to file-based lookup.
            }
            Err(e) => {
                warn!(
                    "TemplatesRepository::get_by_id failed: {}. Falling back to file lookup.",
                    e
                );
            }
        }
    }

    templates::get_template(template_id)
}

/// Split markdown by bold headings (`**Title**`).
///
/// Returns a vector of `(heading_title, content)` pairs, in order. The
/// heading title is the literal text between the `**` markers (e.g.
/// `Resumo`, `Decisões Principais`), preserving the summary's actual
/// language. Callers that previously rendered an English template title
/// should prefer the markdown heading when present so the PDF matches
/// the language of the summary.
///
/// The heading must match the pattern `^[A-ZÀ-Ý][^*]{2,80}$` between the
/// asterisks (capitalized first letter, 2-80 chars, no asterisks inside).
/// This avoids false positives from inline bold text like `**Note:**`.
fn split_markdown_by_bold_headings(markdown: &str) -> Vec<(String, String)> {
    static HEADING_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?m)^\*\*([A-ZÀ-Ý][^*]{2,80})\*\*\s*$").unwrap()
    });

    // (heading_start, heading_end, captured_title) for each bold heading.
    // `captures_iter` runs the regex once and gives us both the match
    // span and the captured group in a single pass — no double-match.
    let headings: Vec<(usize, usize, String)> = HEADING_RE
        .captures_iter(markdown)
        .map(|caps| {
            let m = caps.get(0).expect("captures_iter yields a match");
            let title = caps
                .get(1)
                .map(|c| c.as_str().trim().to_string())
                .unwrap_or_default();
            (m.start(), m.end(), title)
        })
        .collect();

    // Section N's content runs from the end of its heading to the START of
    // the next heading (not the end — that would swallow the next heading).
    let mut sections = Vec::new();
    for (idx, (_, heading_end, heading_title)) in headings.iter().enumerate() {
        let content_end = headings
            .get(idx + 1)
            .map(|&(next_start, _, _)| next_start)
            .unwrap_or(markdown.len());
        sections.push((heading_title.clone(), markdown[*heading_end..content_end].trim().to_string()));
    }

    // If no headings found, return the entire markdown as one section with
    // an empty heading title (caller falls back to template title).
    if sections.is_empty() {
        sections.push((String::new(), markdown.trim().to_string()));
    }

    sections
}

fn merge_sections(template: &Template, summary_result: Option<&str>) -> Vec<SectionContent> {
    let parsed_summary: Option<serde_json::Value> =
        summary_result.and_then(|s| serde_json::from_str(s).ok());

    // Extract markdown from summary (either top-level or english_cache).
    // Prefer the non-English `markdown` field so the PDF is rendered in
    // the summary's actual language; fall back to `english_cache` only
    // when no native-language markdown was stored.
    let markdown = parsed_summary
        .as_ref()
        .and_then(|v| v.get("markdown").and_then(|m| m.as_str()))
        .or_else(|| {
            parsed_summary
                .as_ref()
                .and_then(|v| v.get("english_cache").and_then(|c| c.get("markdown").and_then(|m| m.as_str())))
        });

    // Split markdown by bold headings; each entry carries the actual
    // (translated) heading title so the PDF renders the section heading
    // in the summary's language instead of the English template title.
    let markdown_sections: Vec<(String, String)> =
        markdown.map(split_markdown_by_bold_headings).unwrap_or_default();

    template
        .sections
        .iter()
        .enumerate()
        .map(|(idx, tmpl_section)| {
            let (md_title, content) = markdown_sections
                .get(idx)
                .cloned()
                .unwrap_or((String::new(), "(summary not generated yet)".to_string()));
            // Prefer the markdown-derived (translated) heading title; fall
            // back to the English template title only when the markdown
            // did not provide one (empty string, e.g. LLM omitted the
            // heading or section count mismatch).
            let title = if md_title.is_empty() {
                tmpl_section.title.clone()
            } else {
                md_title
            };
            SectionContent {
                title,
                format: tmpl_section.format.clone(),
                content,
                item_format: tmpl_section
                    .item_format
                    .clone()
                    .or_else(|| tmpl_section.example_item_format.clone()),
            }
        })
        .collect()
}

/// Compute a human-readable duration string for the meeting, if the
/// summary contains `start` / `end` timestamps.
fn compute_duration(summary: Option<&crate::database::models::SummaryProcess>) -> Option<String> {
    let summary = summary?;
    let start = summary.start_time?;
    let end = summary.end_time?;
    let delta = end - start;
    let total = delta.num_seconds().max(0);
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    Some(format!("{:02}:{:02}:{:02}", hours, minutes, seconds))
}

/// Best-effort attendee extraction. Currently a no-op; the source data
/// is not stored, so we return `None` and let the PDF header skip the
/// row. This is the documented behaviour and is intentional.
fn derive_attendees(_summary: Option<&crate::database::models::SummaryProcess>) -> Option<String> {
    None
}

fn build_filename(data: &MeetingExportData) -> String {
    let date_part = data
        .created_at
        .get(..10)
        .unwrap_or("meeting")
        .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "-");
    let title_slug: String = data
        .meeting_title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c.is_whitespace() || c == '-' || c == '_' {
                '-'
            } else {
                // drop diacritics? keep simple: replace with nothing
                '\0'
            }
        })
        .filter(|c| *c != '\0')
        .collect();
    let title_slug = title_slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let title_slug = if title_slug.is_empty() {
        "meeting".to_string()
    } else {
        title_slug
    };
    format!("{}_{}.pdf", date_part, title_slug)
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::templates::TemplateSection;
    use serde_json::json;

    #[test]
    fn filename_strips_unsafe_chars() {
        let data = MeetingExportData {
            meeting_id: "id".into(),
            meeting_title: "Weekly Planning / Q3 #1".into(),
            created_at: "2026-06-29T14:00:00Z".into(),
            duration: None,
            attendees: None,
            template_name: "Standard".into(),
            sections: vec![],
        };
        let name = build_filename(&data);
        assert!(name.starts_with("2026-06-29_"));
        assert!(name.ends_with(".pdf"));
        assert!(!name.contains('/'));
        assert!(!name.contains('#'));
    }

    #[test]
    fn merge_sections_fills_placeholder_when_no_summary() {
        let template = Template {
            name: "T".into(),
            description: "d".into(),
            sections: vec![TemplateSection {
                title: "Summary".into(),
                instruction: "x".into(),
                format: "paragraph".into(),
                item_format: None,
                example_item_format: None,
            }],
        };
        let merged = merge_sections(&template, None);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].title, "Summary");
        assert!(merged[0].content.contains("not generated"));
    }

    #[test]
    fn split_markdown_by_bold_headings_english() {
        let md = "**Summary**\nDiscussed the roadmap.\n**Key Decisions**\n- Decision 1\n**Action Items**\n- Task 1\n";
        let sections = split_markdown_by_bold_headings(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0], ("Summary".to_string(), "Discussed the roadmap.".to_string()));
        assert_eq!(sections[1], ("Key Decisions".to_string(), "- Decision 1".to_string()));
        assert_eq!(sections[2], ("Action Items".to_string(), "- Task 1".to_string()));
    }

    #[test]
    fn split_markdown_by_bold_headings_portuguese() {
        let md = "**Resumo**\nA reunião focou no planejamento.\n**Decisões Principais**\n- Decisão 1\n**Itens de Ação**\n- Tarefa 1\n";
        let sections = split_markdown_by_bold_headings(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0], ("Resumo".to_string(), "A reunião focou no planejamento.".to_string()));
        assert_eq!(sections[1], ("Decisões Principais".to_string(), "- Decisão 1".to_string()));
        assert_eq!(sections[2], ("Itens de Ação".to_string(), "- Tarefa 1".to_string()));
    }

    #[test]
    fn split_markdown_by_bold_headings_ignores_inline_bold() {
        let md = "**Summary**\nThis is a **Note:** inline bold.\n**Key Decisions**\n- Decision 1\n";
        let sections = split_markdown_by_bold_headings(md);
        // Should find 2 headings, not 3 (the inline **Note:** should not match)
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0], ("Summary".to_string(), "This is a **Note:** inline bold.".to_string()));
        assert_eq!(sections[1], ("Key Decisions".to_string(), "- Decision 1".to_string()));
    }

    #[test]
    fn split_markdown_by_bold_headings_empty_content() {
        let md = "**Summary**\n**Key Decisions**\nContent here";
        let sections = split_markdown_by_bold_headings(md);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0], ("Summary".to_string(), "".to_string())); // empty content between headings
        assert_eq!(sections[1], ("Key Decisions".to_string(), "Content here".to_string()));
    }

    #[test]
    fn split_markdown_by_bold_headings_no_headings() {
        let md = "Just some plain text without headings";
        let sections = split_markdown_by_bold_headings(md);
        assert_eq!(sections.len(), 1);
        // No heading found → empty title, full text as content
        assert_eq!(sections[0], ("".to_string(), "Just some plain text without headings".to_string()));
    }

    #[test]
    fn merge_sections_position_based_mapping() {
        let template = Template {
            name: "Test".into(),
            description: "d".into(),
            sections: vec![
                TemplateSection {
                    title: "Summary".into(),
                    instruction: "x".into(),
                    format: "paragraph".into(),
                    item_format: None,
                    example_item_format: None,
                },
                TemplateSection {
                    title: "Key Decisions".into(),
                    instruction: "x".into(),
                    format: "list".into(),
                    item_format: None,
                    example_item_format: None,
                },
                TemplateSection {
                    title: "Action Items".into(),
                    instruction: "x".into(),
                    format: "list".into(),
                    item_format: None,
                    example_item_format: None,
                },
            ],
        };
        let summary = json!({
            "markdown": "**Resumo**\nConteúdo em português.\n**Decisões Principais**\n- Decisão 1\n**Itens de Ação**\n- Tarefa 1"
        });
        let merged = merge_sections(&template, Some(&summary.to_string()));
        assert_eq!(merged.len(), 3);
        // Title must follow the markdown heading (PT), not the English
        // template title. Regression-lock for the "English section names"
        // bug where the translated `**Resumo**` heading was discarded.
        assert_eq!(merged[0].title, "Resumo");
        assert!(merged[0].content.contains("português"));
        assert_eq!(merged[1].title, "Decisões Principais");
        assert!(merged[1].content.contains("Decisão 1"));
        assert_eq!(merged[2].title, "Itens de Ação");
        assert!(merged[2].content.contains("Tarefa 1"));
    }

    #[test]
    fn merge_sections_falls_back_to_template_title_when_markdown_heading_missing() {
        // ponytail: when the LLM omits a heading (or section count
        // mismatches), `merge_sections` must still render SOMETHING for
        // the title — fall back to the English template title rather
        // than emitting an empty heading line in the PDF.
        let template = Template {
            name: "T".into(),
            description: "d".into(),
            sections: vec![TemplateSection {
                title: "Summary".into(),
                instruction: "x".into(),
                format: "paragraph".into(),
                item_format: None,
                example_item_format: None,
            }],
        };
        // Markdown with no bold headings → split returns (empty_title,
        // full_text); merge must fall back to template title.
        let summary = json!({
            "markdown": "Just some content without a heading line"
        });
        let merged = merge_sections(&template, Some(&summary.to_string()));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].title, "Summary");
        assert!(merged[0].content.contains("without a heading"));
    }

    #[test]
    fn merge_sections_threads_item_format_through_to_render() {
        // Locks the item_format threading: merge_sections populates
        // SectionContent.item_format from the template's item_format,
        // falling back to example_item_format when item_format is None.
        // This is the value render_list receives for header synthesis.
        const FMT_PRIMARY: &str = "| **Owner** | **Task** | **Due** |\n| --- | --- | --- |";
        const FMT_FALLBACK: &str = "| **Decision** | **Rationale** |\n| --- | --- |";
        let template = Template {
            name: "Test".into(),
            description: "d".into(),
            sections: vec![
                TemplateSection {
                    title: "Action Items".into(),
                    instruction: "x".into(),
                    format: "list".into(),
                    item_format: Some(FMT_PRIMARY.into()),
                    example_item_format: None,
                },
                TemplateSection {
                    title: "Decisions".into(),
                    instruction: "x".into(),
                    format: "list".into(),
                    item_format: None,
                    example_item_format: Some(FMT_FALLBACK.into()),
                },
            ],
        };
        let merged = merge_sections(&template, None);
        assert_eq!(merged.len(), 2);
        assert_eq!(
            merged[0].item_format.as_deref(),
            Some(FMT_PRIMARY),
            "item_format must come from the template's item_format field"
        );
        assert_eq!(
            merged[1].item_format.as_deref(),
            Some(FMT_FALLBACK),
            "item_format must fall back to example_item_format when item_format is None"
        );
    }

    #[test]
    fn sanitize_filename_replaces_unsafe_chars() {
        let s = sanitize_filename("a/b\\c:d*e?f\"g<h>i|j");
        assert!(!s.contains('/'));
        assert!(!s.contains('\\'));
        assert!(!s.contains(':'));
        assert!(!s.contains('*'));
        assert!(!s.contains('?'));
        assert!(!s.contains('"'));
        assert!(!s.contains('<'));
        assert!(!s.contains('>'));
        assert!(!s.contains('|'));
    }
}