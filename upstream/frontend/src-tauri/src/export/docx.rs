//! DOCX export (placeholder).
//!
//! F2 currently ships PDF export only. The DOCX module is reserved
//! for a future iteration so the export menu has a stable shape and
//! `mod.rs` can expose both entry points without churn.
//!
//! The intent is to render the same `MeetingExportData` consumed by
//! `pdf.rs` using the `docx-rs` crate, reusing the template-driven
//! section layout (title, metadata, sections, action-item table).

use super::pdf::MeetingExportData;

/// Render the meeting data as a DOCX file. Currently unimplemented;
/// the function is kept to make the export surface stable.
pub fn export_meeting_to_docx(_data: &MeetingExportData) -> Result<Vec<u8>, String> {
    Err("DOCX export is not implemented yet. Please use the PDF export option.".to_string())
}
