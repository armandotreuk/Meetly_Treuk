//! Tauri commands for the export module.
//!
//! Currently exposes a single command, `export_meeting_pdf`, that
//! assembles the meeting + summary + template data and returns a PDF
//! rendered by [`crate::export::pdf::export_meeting_to_pdf`].
//!
//! The command is intentionally serializable end-to-end so the
//! frontend can stream the bytes into a `dialog.save` file.

use log::info;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::DialogExt;

use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::summary::{
    resolve_summary_storage_template_id, SummaryProcessesRepository,
};
use crate::export::pdf::{export_meeting_to_pdf, MeetingExportData, SectionContent};
use crate::state::AppState;
use crate::summary::templates::{self, Template, TemplateSection};
use tauri::State;

/// Request payload for the `export_meeting_pdf` command.
///
/// `template_id` is a file/built-in ID or a source-safe database ID such as
/// `db:42`. Unprefixed numeric IDs remain accepted for persisted legacy rows.
#[derive(Debug, Clone, Deserialize)]
pub struct ExportPdfRequest {
    pub meeting_id: String,
    pub template_id: String,
    #[serde(default)]
    pub template_source: Option<String>,
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
    let summary = load_export_summary(pool, &request.meeting_id, &request.template_id).await?;
    let template_reference =
        resolve_export_template_reference(pool, &request.meeting_id, &request.template_id).await?;

    // 3) Resolve the template (DB-stored or built-in). A deleted template
    // reference must not make the stored summary disappear from export.
    let summary_result = summary.as_ref().and_then(|s| s.result.as_deref());
    let template_source = if templates::parse_database_template_id(&template_reference).is_some() {
        Some("database")
    } else if template_reference
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .is_some()
    {
        // Raw numeric references are ambiguous legacy values. The resolver
        // checks DB first and then the numeric file namespace.
        None
    } else {
        request.template_source.as_deref()
    };
    let template =
        match templates::resolve_template_with_source(pool, &template_reference, template_source)
            .await
        {
            Ok(template) => template,
            Err(error) => {
                info!(
                    "Using archived-summary PDF fallback for template {}: {}",
                    template_reference, error
                );
                fallback_template(&template_reference, summary_result)
            }
        };

    // 4) Merge template sections with the stored summary content.
    let sections = merge_sections(&template, summary_result);

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
    let (bytes, page_count) =
        tokio::task::spawn_blocking(move || export_meeting_to_pdf(&data_for_render))
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

async fn load_export_summary(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    template_id: &str,
) -> Result<Option<crate::database::models::SummaryProcess>, String> {
    SummaryProcessesRepository::get_summary_data(pool, meeting_id, template_id)
        .await
        .map_err(|e| format!("Failed to load summary: {}", e))
}

async fn resolve_export_template_reference(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    requested_template_id: &str,
) -> Result<String, String> {
    let storage_template_id =
        resolve_summary_storage_template_id(pool, meeting_id, requested_template_id)
            .await
            .map_err(|e| format!("Failed to resolve summary template reference: {}", e))?;

    // An explicit database reference stays in the strict DB namespace even
    // when its archived summary is stored under the old numeric spelling.
    // This lets the PDF fallback preserve the summary instead of selecting a
    // colliding file template.
    if templates::parse_database_template_id(requested_template_id).is_some() {
        return Ok(requested_template_id.to_string());
    }

    // Raw numeric references predate source-safe IDs and retain the old
    // DB-first/file-second compatibility rule.
    if storage_template_id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .is_some()
    {
        return Ok(storage_template_id);
    }
    Ok(requested_template_id.to_string())
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
    static HEADING_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?m)^\*\*([A-ZÀ-Ý][^*]{2,80})\*\*\s*$").unwrap());

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
    for (idx, (heading_start, heading_end, heading_title)) in headings.iter().enumerate() {
        let content_end = headings
            .get(idx + 1)
            .map(|&(next_start, _, _)| next_start)
            .unwrap_or(markdown.len());
        let content = markdown[*heading_end..content_end].trim();
        let leading_content = if idx == 0 {
            markdown[..*heading_start].trim()
        } else {
            ""
        };
        let content = if leading_content.is_empty() {
            content.to_string()
        } else if content.is_empty() {
            leading_content.to_string()
        } else {
            format!("{leading_content}\n\n{content}")
        };
        sections.push((heading_title.clone(), content));
    }

    // If no headings found, return the entire markdown as one section with
    // an empty heading title (caller falls back to template title).
    if sections.is_empty() {
        sections.push((String::new(), markdown.trim().to_string()));
    }

    sections
}

fn extract_meeting_notes_sections(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Vec<(String, String)> {
    let Some(sections) = object
        .get("MeetingNotes")
        .and_then(|notes| notes.get("sections"))
        .and_then(|sections| sections.as_array())
    else {
        return Vec::new();
    };

    sections
        .iter()
        .enumerate()
        .filter_map(|(index, section)| {
            let section_object = section.as_object()?;
            let key = section_object
                .get("title")
                .and_then(|value| value.as_str())
                .filter(|title| !title.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("Section {}", index + 1));
            let title = section_object
                .get("title")
                .and_then(|value| value.as_str())
                .unwrap_or(&key)
                .to_string();
            let content = section_object
                .get("blocks")
                .and_then(|value| value.as_array())
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| {
                            block.get("content").map(|content| {
                                content
                                    .as_str()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| content.to_string())
                            })
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .or_else(|| {
                    section_object
                        .get("content")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_default();
            Some((title, content))
        })
        .collect()
}

fn merge_summary_sources(
    mut primary: Vec<(String, String)>,
    notes: Vec<(String, String)>,
) -> Vec<(String, String)> {
    for note in notes {
        if let Some(existing) = primary
            .iter_mut()
            .find(|(title, _)| title.trim().eq_ignore_ascii_case(note.0.trim()))
        {
            // MeetingNotes is the editor-owned representation; retain it
            // when a cached markdown representation has the same heading.
            *existing = note;
        } else {
            primary.push(note);
        }
    }
    primary
}

fn extract_summary_sections(summary_result: Option<&str>) -> Vec<(String, String)> {
    let Some(raw) = summary_result else {
        return Vec::new();
    };

    let parsed_summary = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => value,
        Err(_) => return vec![(String::new(), raw.trim().to_string())],
    };

    // Older rows can contain a JSON-encoded markdown string instead of an
    // object. Treat it as markdown rather than exporting the JSON quotes.
    if let Some(markdown) = parsed_summary.as_str() {
        return split_markdown_by_bold_headings(markdown);
    }

    // Prefer the non-English markdown, then the cached English markdown, but
    // do not return before merging editor-owned MeetingNotes.sections below.
    let markdown_sections = parsed_summary
        .get("markdown")
        .and_then(|value| value.as_str())
        .filter(|markdown| !markdown.trim().is_empty())
        .map(split_markdown_by_bold_headings);
    let cached_sections = parsed_summary
        .get("english_cache")
        .and_then(|cache| cache.get("markdown"))
        .and_then(|value| value.as_str())
        .filter(|markdown| !markdown.trim().is_empty())
        .map(split_markdown_by_bold_headings);

    // Legacy summaries store sections as `{ title, blocks }` objects.
    let Some(object) = parsed_summary.as_object() else {
        return markdown_sections
            .or(cached_sections)
            .unwrap_or_else(|| vec![(String::new(), raw.trim().to_string())]);
    };
    let notes_sections = extract_meeting_notes_sections(object);
    if let Some(markdown_sections) = markdown_sections {
        return merge_summary_sources(markdown_sections, notes_sections);
    }
    if let Some(cached_sections) = cached_sections {
        return merge_summary_sources(cached_sections, notes_sections);
    }

    let mut sections_to_extract: Vec<(String, &serde_json::Value)> = Vec::new();
    if let Some(sections) = object
        .get("MeetingNotes")
        .and_then(|notes| notes.get("sections"))
        .and_then(|sections| sections.as_array())
    {
        // Legacy editor saves use MeetingNotes.sections instead of keyed
        // top-level sections. Keep their stored order and block shape.
        for (index, section) in sections.iter().enumerate() {
            let key = section
                .get("title")
                .and_then(|value| value.as_str())
                .filter(|title| !title.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("Section {}", index + 1));
            sections_to_extract.push((key, section));
        }
    } else {
        let mut keys = Vec::new();
        if let Some(order) = object
            .get("_section_order")
            .and_then(|value| value.as_array())
        {
            keys.extend(
                order
                    .iter()
                    .filter_map(|key| key.as_str().map(str::to_string)),
            );
        }
        for key in object.keys() {
            if key != "MeetingName"
                && key != "_section_order"
                && key != "english_cache"
                && key != "MeetingNotes"
                && !keys.contains(key)
            {
                keys.push(key.clone());
            }
        }
        sections_to_extract.extend(
            keys.into_iter()
                .filter_map(|key| object.get(&key).map(|section| (key, section))),
        );
    }

    let mut sections = Vec::new();
    for (key, section) in sections_to_extract {
        let Some(section_object) = section.as_object() else {
            continue;
        };
        let title = section_object
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or(&key)
            .to_string();
        let content = section_object
            .get("blocks")
            .and_then(|value| value.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|block| {
                        block.get("content").map(|content| {
                            content
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| content.to_string())
                        })
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .or_else(|| {
                section_object
                    .get("content")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        sections.push((title, content));
    }

    if sections.is_empty() {
        vec![(
            String::new(),
            serde_json::to_string_pretty(&parsed_summary).unwrap_or_else(|_| raw.to_string()),
        )]
    } else {
        sections
    }
}

fn fallback_template(template_id: &str, summary_result: Option<&str>) -> Template {
    let summary_sections = extract_summary_sections(summary_result);
    let sections = if summary_sections.is_empty() {
        vec![TemplateSection {
            title: "Summary".to_string(),
            instruction: "Preserve the stored summary content".to_string(),
            format: "paragraph".to_string(),
            item_format: None,
            example_item_format: None,
        }]
    } else {
        summary_sections
            .iter()
            .enumerate()
            .map(|(index, (title, _))| TemplateSection {
                title: if title.is_empty() {
                    format!("Summary {}", index + 1)
                } else {
                    title.clone()
                },
                instruction: "Preserve the stored summary content".to_string(),
                format: "paragraph".to_string(),
                item_format: None,
                example_item_format: None,
            })
            .collect()
    };

    Template {
        name: format!("Archived summary ({})", template_id),
        description: "Fallback template for a legacy or deleted template reference".to_string(),
        sections,
    }
}

fn merge_sections(template: &Template, summary_result: Option<&str>) -> Vec<SectionContent> {
    let summary_sections = extract_summary_sections(summary_result);

    // Use the larger section count. A deleted/changed template must not make
    // stored MeetingNotes sections disappear from an export.
    let section_count = template.sections.len().max(summary_sections.len());
    (0..section_count)
        .map(|idx| {
            let tmpl_section = template.sections.get(idx);
            let (md_title, content) = summary_sections
                .get(idx)
                .cloned()
                .unwrap_or((String::new(), "(summary not generated yet)".to_string()));
            let title = if !md_title.is_empty() {
                md_title
            } else if let Some(tmpl_section) = tmpl_section {
                tmpl_section.title.clone()
            } else {
                format!("Section {}", idx + 1)
            };
            SectionContent {
                title,
                format: tmpl_section
                    .map(|section| section.format.clone())
                    .unwrap_or_else(|| "paragraph".to_string()),
                content,
                item_format: tmpl_section.and_then(|section| {
                    section
                        .item_format
                        .clone()
                        .or_else(|| section.example_item_format.clone())
                }),
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
    use crate::database::repositories::summary::canonical_summary_template_id;
    use crate::summary::templates::TemplateSection;
    use serde_json::json;

    #[test]
    fn summary_identity_canonicalizes_legacy_numeric_ids_without_touching_files() {
        assert_eq!(canonical_summary_template_id("42"), "db:42");
        assert_eq!(canonical_summary_template_id("db:42"), "db:42");
        assert_eq!(canonical_summary_template_id("file:42"), "file:42");
        assert_eq!(
            canonical_summary_template_id("standard_meeting"),
            "standard_meeting"
        );
        assert!(templates::parse_database_template_id("db:42").is_some());
        assert!(templates::parse_database_template_id("file:42").is_none());
    }

    #[tokio::test]
    async fn export_summary_lookup_bridges_numeric_legacy_row() {
        let pool = sqlx::SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            "CREATE TABLE summary_processes (meeting_id TEXT NOT NULL, template_id TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, error TEXT, result TEXT, start_time TEXT, end_time TEXT, chunk_count INTEGER DEFAULT 0, processing_time REAL DEFAULT 0.0, metadata TEXT, result_backup TEXT, result_backup_timestamp TEXT, PRIMARY KEY (meeting_id, template_id))",
        )
        .execute(&pool)
        .await
        .expect("create summary schema");
        let now = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("meeting-1")
        .bind("42")
        .bind("completed")
        .bind(now)
        .bind(now)
        .bind(r#"{"MeetingNotes":{"sections":[]}}"#)
        .execute(&pool)
        .await
        .expect("insert legacy summary");

        let summary = load_export_summary(&pool, "meeting-1", "db:42")
            .await
            .expect("load export summary")
            .expect("legacy export summary");
        assert_eq!(summary.template_id, "db:42");
    }

    #[tokio::test]
    async fn explicit_database_reference_uses_archived_fallback_instead_of_colliding_file() {
        let pool = sqlx::SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            "CREATE TABLE summary_processes (meeting_id TEXT NOT NULL, template_id TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, error TEXT, result TEXT, start_time TEXT, end_time TEXT, chunk_count INTEGER DEFAULT 0, processing_time REAL DEFAULT 0.0, metadata TEXT, result_backup TEXT, result_backup_timestamp TEXT, PRIMARY KEY (meeting_id, template_id))",
        )
        .execute(&pool)
        .await
        .expect("create summary schema");
        sqlx::query(
            "CREATE TABLE templates (id INTEGER PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL, stable_id TEXT, schema_json TEXT NOT NULL, is_builtin INTEGER NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .expect("create template schema");
        let now = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("meeting-legacy-file")
        .bind("987655")
        .bind("completed")
        .bind(now)
        .bind(now)
        .bind(r#"{"markdown":"Stored content"}"#)
        .execute(&pool)
        .await
        .expect("insert legacy numeric summary");

        let dir = tempfile::tempdir().expect("create bundled template directory");
        std::fs::write(
            dir.path().join("987655.json"),
            r#"{
                "name": "Legacy file template",
                "description": "File fallback",
                "sections": [{
                    "title": "Summary",
                    "instruction": "Preserve",
                    "format": "paragraph"
                }]
            }"#,
        )
        .expect("write file template");
        let _lock = crate::summary::templates::acquire_template_test_lock();
        templates::set_bundled_templates_dir(dir.path().to_path_buf());

        let reference =
            resolve_export_template_reference(&pool, "meeting-legacy-file", "db:987655")
                .await
                .expect("resolve stored reference");
        assert_eq!(reference, "db:987655");
        let _ = templates::resolve_template(&pool, &reference)
            .await
            .expect_err("explicit DB reference must remain strict");
        let _ = templates::resolve_template_with_source(&pool, &reference, Some("database"))
            .await
            .expect_err("export DB source must remain strict");
        let template = fallback_template(&reference, Some(r#"{"markdown":"Stored content"}"#));
        let sections = merge_sections(&template, Some(r#"{"markdown":"Stored content"}"#));
        assert!(sections[0].content.contains("Stored content"));
    }

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
    fn fallback_template_is_valid_without_summary() {
        let template = fallback_template("deleted", None);

        assert!(template.validate().is_ok());
        assert_eq!(template.sections.len(), 1);
    }

    #[test]
    fn archived_legacy_summary_content_survives_template_fallback() {
        let raw = json!({
            "MeetingName": "Old meeting",
            "_section_order": ["key_points", "actions"],
            "key_points": {
                "title": "Key points",
                "blocks": [{"content": "Decision preserved"}]
            },
            "actions": {
                "title": "Actions",
                "blocks": [{"content": "Follow-up preserved"}]
            }
        })
        .to_string();

        let template = fallback_template("legacy", Some(&raw));
        let sections = merge_sections(&template, Some(&raw));

        assert_eq!(sections.len(), 2);
        assert!(sections[0].content.contains("Decision preserved"));
        assert!(sections[1].content.contains("Follow-up preserved"));
    }

    #[test]
    fn archived_meeting_notes_sections_reach_pdf_input() {
        let raw = json!({
            "MeetingName": "Old meeting",
            "MeetingNotes": {
                "sections": [
                    {
                        "title": "Decisions",
                        "blocks": [{"content": "Decision from notes"}]
                    },
                    {
                        "title": "Actions",
                        "blocks": [{"content": "Action from notes"}]
                    }
                ]
            }
        })
        .to_string();

        let sections = merge_sections(&fallback_template("deleted", Some(&raw)), Some(&raw));

        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "Decisions");
        assert!(sections[0].content.contains("Decision from notes"));
        assert_eq!(sections[1].title, "Actions");
        assert!(sections[1].content.contains("Action from notes"));
    }

    #[test]
    fn meeting_notes_sections_survive_markdown_and_english_cache_fields() {
        let raw = json!({
            "markdown": "**Summary**\nCached summary.",
            "english_cache": {"markdown": "**Summary**\nEnglish cache."},
            "MeetingNotes": {
                "sections": [
                    {
                        "title": "Edited decisions",
                        "blocks": [{"content": "Decision edited by user"}]
                    }
                ]
            }
        })
        .to_string();

        let sections = extract_summary_sections(Some(&raw));

        assert!(sections
            .iter()
            .any(|(title, content)| { title == "Summary" && content.contains("Cached summary") }));
        assert!(sections.iter().any(|(title, content)| {
            title == "Edited decisions" && content.contains("Decision edited by user")
        }));
    }

    #[test]
    fn changed_embedded_template_preserves_extra_stored_meeting_notes_sections() {
        let raw = json!({
            "MeetingNotes": {
                "sections": [
                    {
                        "title": "Stored first",
                        "blocks": [{"content": "First section"}]
                    },
                    {
                        "title": "Stored second",
                        "blocks": [{"content": "Second section"}]
                    }
                ]
            }
        })
        .to_string();
        let template = Template {
            name: "Current embedded schema".into(),
            description: "Only one current section".into(),
            sections: vec![TemplateSection {
                title: "Current first".into(),
                instruction: "Keep content".into(),
                format: "paragraph".into(),
                item_format: None,
                example_item_format: None,
            }],
        };

        let sections = merge_sections(&template, Some(&raw));

        assert_eq!(sections.len(), 2);
        assert!(sections.iter().any(|section| {
            section.title == "Stored first" && section.content.contains("First section")
        }));
        assert!(sections.iter().any(|section| {
            section.title == "Stored second" && section.content.contains("Second section")
        }));
    }

    #[test]
    fn deleted_template_fallback_preserves_all_markdown_sections() {
        let raw = json!({
            "markdown": "**Summary**\nKept summary.\n**Actions**\nKept action."
        })
        .to_string();

        let template = fallback_template("42", Some(&raw));
        let sections = merge_sections(&template, Some(&raw));

        assert_eq!(sections.len(), 2);
        assert!(sections
            .iter()
            .any(|section| section.content == "Kept summary."));
        assert!(sections
            .iter()
            .any(|section| section.content == "Kept action."));
    }

    #[test]
    fn fallback_preserves_json_encoded_markdown_and_leading_content() {
        let markdown = "# Stored title\n\n**Summary**\nStored body.";
        let raw = serde_json::to_string(markdown).expect("encode markdown");

        let sections = merge_sections(&fallback_template("deleted", Some(&raw)), Some(&raw));

        assert_eq!(sections.len(), 1);
        assert!(sections[0].content.contains("# Stored title"));
        assert!(sections[0].content.contains("Stored body."));
    }

    #[test]
    fn split_markdown_by_bold_headings_english() {
        let md = "**Summary**\nDiscussed the roadmap.\n**Key Decisions**\n- Decision 1\n**Action Items**\n- Task 1\n";
        let sections = split_markdown_by_bold_headings(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(
            sections[0],
            ("Summary".to_string(), "Discussed the roadmap.".to_string())
        );
        assert_eq!(
            sections[1],
            ("Key Decisions".to_string(), "- Decision 1".to_string())
        );
        assert_eq!(
            sections[2],
            ("Action Items".to_string(), "- Task 1".to_string())
        );
    }

    #[test]
    fn split_markdown_by_bold_headings_portuguese() {
        let md = "**Resumo**\nA reunião focou no planejamento.\n**Decisões Principais**\n- Decisão 1\n**Itens de Ação**\n- Tarefa 1\n";
        let sections = split_markdown_by_bold_headings(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(
            sections[0],
            (
                "Resumo".to_string(),
                "A reunião focou no planejamento.".to_string()
            )
        );
        assert_eq!(
            sections[1],
            ("Decisões Principais".to_string(), "- Decisão 1".to_string())
        );
        assert_eq!(
            sections[2],
            ("Itens de Ação".to_string(), "- Tarefa 1".to_string())
        );
    }

    #[test]
    fn split_markdown_by_bold_headings_ignores_inline_bold() {
        let md = "**Summary**\nThis is a **Note:** inline bold.\n**Key Decisions**\n- Decision 1\n";
        let sections = split_markdown_by_bold_headings(md);
        // Should find 2 headings, not 3 (the inline **Note:** should not match)
        assert_eq!(sections.len(), 2);
        assert_eq!(
            sections[0],
            (
                "Summary".to_string(),
                "This is a **Note:** inline bold.".to_string()
            )
        );
        assert_eq!(
            sections[1],
            ("Key Decisions".to_string(), "- Decision 1".to_string())
        );
    }

    #[test]
    fn split_markdown_by_bold_headings_empty_content() {
        let md = "**Summary**\n**Key Decisions**\nContent here";
        let sections = split_markdown_by_bold_headings(md);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0], ("Summary".to_string(), "".to_string())); // empty content between headings
        assert_eq!(
            sections[1],
            ("Key Decisions".to_string(), "Content here".to_string())
        );
    }

    #[test]
    fn split_markdown_by_bold_headings_no_headings() {
        let md = "Just some plain text without headings";
        let sections = split_markdown_by_bold_headings(md);
        assert_eq!(sections.len(), 1);
        // No heading found → empty title, full text as content
        assert_eq!(
            sections[0],
            (
                "".to_string(),
                "Just some plain text without headings".to_string()
            )
        );
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
