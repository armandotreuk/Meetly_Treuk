//! Export module - PDF and DOCX export functionality
//!
//! This module provides export capabilities for meeting summaries using
//! templates for consistent formatting.
//!
//! See [`pdf::export_meeting_to_pdf`] for the canonical entry point
//! used by the Tauri command in [`commands`].

pub mod commands;
pub mod context;
pub mod docx;
pub mod pdf;

pub use commands::{export_meeting_pdf, save_meeting_pdf, ExportPdfRequest, ExportPdfResponse};
pub use context::{build_context_markdown, build_context_markdown_with_limit};
pub use pdf::{export_meeting_to_pdf, MeetingExportData, SectionContent};
