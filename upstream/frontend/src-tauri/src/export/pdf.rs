//! PDF export for meeting summaries.
//!
//! Produces a template-driven A4 PDF that embeds the meeting metadata
//! (title, date, attendees) and renders each section defined in the
//! selected template (paragraph / list / string).
//!
//! ## Fonts and Unicode (PT-BR / accents)
//!
//! `printpdf`'s built-in PDF fonts (Helvetica, Times, Courier) only
//! support WinAnsi encoding, so accented characters like `ç`, `ã`,
//! `é` and `í` would be lost. To render PT-BR text correctly, the
//! generator embeds an external TrueType font that covers the Latin
//! Extended-A range.
//!
//! The lookup order is:
//! 1. An explicit override via the `MEETLY_PDF_FONT_DIR` env var.
//! 2. The `templates/fonts/` directory bundled next to the binary.
//! 3. The platform system font directory (Windows / macOS / Linux).
//! 4. A graceful fallback to Helvetica, with a warning log if the
//!    document contains non-ASCII characters.
//!
//! Accepted file names (case-insensitive):
//! - `DejaVuSans.ttf` and `DejaVuSans-Bold.ttf` (preferred)
//! - `NotoSans-Regular.ttf` and `NotoSans-Bold.ttf`
//! - `Arial.ttf` and `Arial-Bold.ttf`
//!
//! If only the regular weight is present, the bold slot silently
//! falls back to the regular weight (no glyph substitution).
//!
//! ## Layout
//!
//! - A4 portrait, 20 mm margins.
//! - Title (18 pt, bold), metadata (10 pt), divider line, then one
//!   block per template section with a heading and body.
//! - Text is word-wrapped to fit the printable width using a manual
//!   `printpdf` text layer; new pages are inserted automatically.

use printpdf::{
    BuiltinFont, IndirectFontRef, Line, Mm, PdfDocument, PdfDocumentReference, PdfLayerIndex,
    PdfLayerReference, PdfPageIndex, Point,
};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use tracing::{info, warn};

// ---------- Public data types ----------

/// Section content produced by merging a template with a summary.
///
/// The `format` field mirrors the template's `format` value
/// (`paragraph`, `list`, or `string`) so the renderer can choose the
/// correct layout without re-parsing the template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionContent {
    pub title: String,
    pub format: String,
    pub content: String,
    /// Optional `item_format` hint from the template (e.g. a markdown
    /// table header). Used to emit table-style rows when present.
    #[serde(default)]
    pub item_format: Option<String>,
}

/// Input bundle for the PDF generator. Constructed by the Tauri
/// command that wires the database row + the template together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingExportData {
    pub meeting_id: String,
    pub meeting_title: String,
    /// ISO 8601 timestamp of when the meeting was created.
    pub created_at: String,
    /// Optional meeting duration string (e.g. "00:42:13").
    #[serde(default)]
    pub duration: Option<String>,
    /// Optional attendee list, free-form (one per line or comma separated).
    #[serde(default)]
    pub attendees: Option<String>,
    /// Display name of the template that produced the summary.
    pub template_name: String,
    /// Sections, in template order.
    pub sections: Vec<SectionContent>,
}

// ---------- Public entry point ----------

/// Render the supplied meeting data as a PDF and return the bytes.
///
/// This function does not touch the filesystem: callers are responsible
/// for writing the bytes to disk (or piping them through a Tauri
/// `dialog.save` flow on the frontend).
pub fn export_meeting_to_pdf(data: &MeetingExportData) -> Result<(Vec<u8>, usize), String> {
    let (doc, page, layer) = PdfDocument::new(
        format!("Meeting: {}", data.meeting_title),
        Mm(PAGE_WIDTH_MM),
        Mm(PAGE_HEIGHT_MM),
        "page-1",
    );

    let fonts = FontSet::load(&doc)?;
    let mut ctx = RenderContext::new(&doc, page, layer, fonts, data.meeting_title.clone());

    render_header(&mut ctx, data)?;
    render_metadata(&mut ctx, data)?;
    render_divider(&mut ctx)?;
    render_sections(&mut ctx, data)?;
    // Render footer on the last page
    ctx.render_footer_inline();

    let page_count = ctx.page_number;

    // Release the `&doc` borrow held by `ctx` before consuming the
    // document. This keeps NLL happy even on older compilers.
    drop(ctx);

    // `doc` has no other owners at this point, so `save_to_bytes` can
    // consume it and serialize the document.
    let bytes = doc
        .save_to_bytes()
        .map_err(|e| format!("Failed to serialize PDF: {}", e))?;

    info!(
        "Generated PDF for meeting '{}' ({} bytes, {} sections, {} pages)",
        data.meeting_title,
        bytes.len(),
        data.sections.len(),
        page_count
    );

    Ok((bytes, page_count))
}

// ---------- Rendering context ----------

struct RenderContext<'a> {
    doc: &'a PdfDocumentReference,
    page: PdfPageIndex,
    layer: PdfLayerIndex,
    fonts: FontSet,
    /// Current cursor measured from the top edge, in mm.
    cursor_y: f64,
    page_number: usize,
    meeting_title: String,
}

#[derive(Clone)]
struct FontSet {
    regular: FontHandle,
    bold: FontHandle,
}

#[derive(Clone)]
enum FontHandle {
    External(IndirectFontRef),
    Builtin(BuiltinFont),
}

impl FontSet {
    fn load(doc: &PdfDocumentReference) -> Result<Self, String> {
        let regular = load_unicode_font(doc, FontWeight::Regular)?;
        let bold = match load_unicode_font(doc, FontWeight::Bold) {
            Ok(b) => b,
            Err(_) => regular.clone(),
        };

        Ok(Self { regular, bold })
    }

    fn regular(&self) -> FontHandle {
        self.regular.clone()
    }

    fn bold(&self) -> FontHandle {
        self.bold.clone()
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum FontWeight {
    Regular,
    Bold,
}

// ---------- Layout constants ----------

const MARGIN_MM: f64 = 20.0;
const PAGE_WIDTH_MM: f64 = 210.0;
const PAGE_HEIGHT_MM: f64 = 297.0;
const CONTENT_TOP_MM: f64 = PAGE_HEIGHT_MM - MARGIN_MM;
const CONTENT_BOTTOM_MM: f64 = MARGIN_MM + 10.0; // leave room for footer
const CONTENT_LEFT_MM: f64 = MARGIN_MM;
const CONTENT_RIGHT_MM: f64 = PAGE_WIDTH_MM - MARGIN_MM;
const CONTENT_WIDTH_MM: f64 = CONTENT_RIGHT_MM - CONTENT_LEFT_MM;

const TITLE_SIZE_PT: f64 = 18.0;
const META_SIZE_PT: f64 = 10.0;
const SECTION_HEADING_SIZE_PT: f64 = 13.0;
const BODY_SIZE_PT: f64 = 10.5;
const FOOTER_SIZE_PT: f64 = 8.0;

const FOOTER_Y_MM: f64 = MARGIN_MM; // 20mm from bottom

const LINE_HEIGHT_BODY: f64 = 4.6;
const LINE_HEIGHT_HEADING: f64 = 6.0;
const LINE_HEIGHT_TITLE: f64 = 8.0;
const SECTION_SPACING_MM: f64 = 5.0;
const PARAGRAPH_SPACING_MM: f64 = 2.0;

const TABLE_MIN_COL_WIDTH_MM: f64 = 18.0;
const TABLE_HEADER_PADDING_MM: f64 = 1.5;
const TABLE_CELL_PADDING_MM: f64 = 1.5;
const TABLE_BORDER_WIDTH_PT: f64 = 0.5;
const TABLE_HEADER_REPEAT: bool = true;

// ---------- Rendering helpers ----------

impl<'a> RenderContext<'a> {
    fn new(
        doc: &'a PdfDocumentReference,
        page: PdfPageIndex,
        layer: PdfLayerIndex,
        fonts: FontSet,
        meeting_title: String,
    ) -> Self {
        Self {
            doc,
            page,
            layer,
            fonts,
            cursor_y: CONTENT_TOP_MM,
            page_number: 1,
            meeting_title,
        }
    }

    fn layer_ref(&self) -> PdfLayerReference {
        self.doc.get_page(self.page).get_layer(self.layer)
    }

    /// Reserve vertical space; insert a page break if there isn't
    /// enough room for `needed_mm` before the footer area.
    fn ensure_space(&mut self, needed_mm: f64) {
        if self.cursor_y - needed_mm < CONTENT_BOTTOM_MM {
            self.new_page();
        }
    }

    fn new_page(&mut self) {
        // Render footer on the page we're leaving
        self.render_footer_inline();
        let (page, layer) = self.doc.add_page(
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            format!("page-{}", self.page_number + 1),
        );
        self.page = page;
        self.layer = layer;
        self.page_number += 1;
        self.cursor_y = CONTENT_TOP_MM;
    }

    /// Render footer on the current page (used when advancing to next page)
    fn render_footer_inline(&self) {
        let layer = self.layer_ref();
        let footer = format!(
            "Personal Meetly • {} • Page {}",
            self.meeting_title, self.page_number
        );
        if let Ok(font) = resolve_font(self, FontWeight::Regular) {
            layer.use_text(
                &footer,
                FOOTER_SIZE_PT,
                Mm(CONTENT_LEFT_MM),
                Mm(FOOTER_Y_MM),
                &font,
            );
        }
    }

    /// Write a single line of text at the current cursor and advance.
    fn write_line(&mut self, text: &str, size_pt: f64, weight: FontWeight) {
        let handle = match weight {
            FontWeight::Bold => self.fonts.bold(),
            FontWeight::Regular => self.fonts.regular(),
        };
        let layer = self.layer_ref();

        match handle {
            FontHandle::External(ref f) => {
                layer.use_text(text, size_pt, Mm(CONTENT_LEFT_MM), Mm(self.cursor_y), f);
            }
            FontHandle::Builtin(b) => {
                if let Ok(f) = self.doc.add_builtin_font(b) {
                    layer.use_text(text, size_pt, Mm(CONTENT_LEFT_MM), Mm(self.cursor_y), &f);
                }
            }
        }

        let line_height = if size_pt >= SECTION_HEADING_SIZE_PT {
            LINE_HEIGHT_HEADING
        } else if size_pt >= TITLE_SIZE_PT {
            LINE_HEIGHT_TITLE
        } else {
            LINE_HEIGHT_BODY
        };
        self.cursor_y -= line_height;
    }

    /// Word-wrap a paragraph to the printable width, then write each
    /// line. Approximates per-glyph width with a 0.5 × size heuristic
    /// which is close enough for DejaVu Sans / Arial across Latin
    /// scripts.
    fn write_wrapped(&mut self, text: &str, size_pt: f64, weight: FontWeight) {
        let y = self._write_wrapped_impl(
            text,
            size_pt,
            weight,
            CONTENT_LEFT_MM,
            self.cursor_y,
            CONTENT_WIDTH_MM,
        );
        self.cursor_y = y;
    }

    /// Write a single line of text at explicit (x_mm, y_mm); does not
    /// touch `cursor_y`.
    fn write_line_at(
        &mut self,
        text: &str,
        size_pt: f64,
        weight: FontWeight,
        x_mm: f64,
        y_mm: f64,
    ) {
        let handle = match weight {
            FontWeight::Bold => self.fonts.bold(),
            FontWeight::Regular => self.fonts.regular(),
        };
        let layer = self.layer_ref();
        match handle {
            FontHandle::External(ref f) => {
                layer.use_text(text, size_pt, Mm(x_mm), Mm(y_mm), f);
            }
            FontHandle::Builtin(b) => {
                if let Ok(f) = self.doc.add_builtin_font(b) {
                    layer.use_text(text, size_pt, Mm(x_mm), Mm(y_mm), &f);
                }
            }
        }
    }

    /// Write a line split into `(weight, text)` segments, advancing `x_mm`
    /// by each segment's measured width using the bundled-font `hmtx`.
    /// Used for inline markdown `**bold**` rendering. Segments with no
    /// measured metrics fall back to writing sequentially without
    /// per-segment x-advance (which can overlap; acceptable for the
    /// rare no-font-metrics code path where the builtin Helvetica is
    /// used and there's no per-glyph width to advance by — the
    /// alternative is hand-coded Helvetica AFM metrics, see ponytail).
    fn write_line_at_segments(
        &mut self,
        segments: &[(FontWeight, &str)],
        size_pt: f64,
        x_mm: f64,
        y_mm: f64,
    ) {
        let mut x = x_mm;
        let reg = font_metrics();
        let bold = bold_font_metrics();
        for (weight, text) in segments {
            let text: &str = *text;
            if text.is_empty() {
                continue;
            }
            let handle = match weight {
                FontWeight::Bold => self.fonts.bold(),
                FontWeight::Regular => self.fonts.regular(),
            };
            let layer = self.layer_ref();
            match handle {
                FontHandle::External(ref f) => {
                    layer.use_text(text, size_pt, Mm(x), Mm(y_mm), f);
                }
                FontHandle::Builtin(b) => {
                    if let Ok(f) = self.doc.add_builtin_font(b) {
                        layer.use_text(text, size_pt, Mm(x), Mm(y_mm), &f);
                    }
                }
            }
            // Advance x by the segment's measured width using the metrics
            // for its own weight: Bold segments with bold metrics, Regular
            // with regular. If the bold TTF is missing, `bold` is None and
            // we fall back to `reg` (the previous, overlap-prone behaviour;
            // the bold glyphs are actually drawn with the regular font in
            // that case too, so regular metrics happen to be correct then).
            let m = match weight {
                FontWeight::Bold => bold.or(reg),
                FontWeight::Regular => reg,
            };
            if let Some(metrics) = m {
                x += metrics.text_width_mm(text, size_pt);
            }
        }
    }

    /// Word-wrap `text` to `max_width_mm` and write each line at
    /// `x_mm` starting from `start_y_mm`, triggering a page break if
    /// a line would run past the footer. Returns the y of the next
    /// free line below the block. Does not touch `cursor_y`.
    fn _write_wrapped_impl(
        &mut self,
        text: &str,
        size_pt: f64,
        weight: FontWeight,
        x_mm: f64,
        start_y_mm: f64,
        max_width_mm: f64,
    ) -> f64 {
        self._write_wrapped_impl_opts(text, size_pt, weight, x_mm, start_y_mm, max_width_mm, true)
    }

    /// Same as `_write_wrapped_impl` but with a `allow_paginate` flag.
    /// When `false` (e.g. called from `draw_table_row`), no page break
    /// is inserted: the caller has already verified (via `compute_row_height`)
    /// that every line fits within the row's borders — and `compute_row_height`
    /// uses the same `wrap_to_width` call, so the prediction is exact.
    /// Paginating mid-cell would split the row across pages with the
    /// cell text on a fresh page but the cell borders on the old one
    /// (the "lone text without borders" + "doubled header" bug).
    /// When `allow_paginate` is `false` we still defend against the
    /// catastrophic case where `compute_row_height` under-predicted:
    /// if any line would drop below `CONTENT_BOTTOM_MM`, fall back to
    /// hard-clipping (skip writing that line). This avoids drawing into
    /// the footer / off-page; data preservation is the caller's
    /// responsibility (would already mean a measurement bug we want
    /// surfaced).
    fn _write_wrapped_impl_opts(
        &mut self,
        text: &str,
        size_pt: f64,
        weight: FontWeight,
        x_mm: f64,
        start_y_mm: f64,
        max_width_mm: f64,
        allow_paginate: bool,
    ) -> f64 {
        let line_height = if size_pt >= TITLE_SIZE_PT {
            LINE_HEIGHT_TITLE
        } else if size_pt >= SECTION_HEADING_SIZE_PT {
            LINE_HEIGHT_HEADING
        } else {
            LINE_HEIGHT_BODY
        };
        // ponytail: 0.8em / 0.2em ascent/descent split matches the
        // `cell_y` heuristic in `draw_table_row` and `compute_row_height`.
        // `descent` is only needed in the no-pagination path (cell-guard
        // check); kept out of the preemptive-pagination path to avoid a
        // dead-variable lint. `ascent` is unused here — the draw position
        // is supplied by `cell_y` from `draw_table_row`.
        let descent = size_pt * 0.3528 * 0.2;
        let mut y = start_y_mm;
        for paragraph in split_paragraphs(text) {
            for line in wrap_to_width(&paragraph, max_width_mm, size_pt) {
                if allow_paginate {
                    // Preemptive pagination for body paragraphs: if writing
                    // THIS line would push the NEXT one below the floor,
                    // move to a new page now so consecutive lines stay
                    // together. (Otherwise the next iteration's line
                    // would land past the page bottom.)
                    if y - line_height < CONTENT_BOTTOM_MM {
                        self.new_page();
                        y = CONTENT_TOP_MM;
                    }
                } else {
                    // Inside a table cell: never paginate (the row's
                    // pre-check already guaranteed the row fits on this
                    // page, and `compute_row_height` uses the same
                    // `wrap_to_width` so every line stays within the
                    // cell borders). The guard below defends only
                    // against a measurement-vs-draw divergence that
                    // would push a line below the page bottom margin —
                    // skip silently rather than corrupt the table layout.
                    if y - descent < CONTENT_BOTTOM_MM {
                        // ponytail: ceiling — silent drop. Upgrade path:
                        // treat as a measurement bug in `compute_row_height`
                        // rather than paging mid-row.
                        break;
                    }
                }
                let segments = split_bold_segments(&line, weight);
                // Fast path: single Regular segment means no `**bold**`
                // markers in this line — write as one shot (no
                // per-segment x-advance overhead).
                if segments.len() == 1 {
                    let (w, t) = segments[0];
                    self.write_line_at(t, size_pt, w, x_mm, y);
                } else {
                    self.write_line_at_segments(&segments, size_pt, x_mm, y);
                }
                y -= line_height;
            }
            y -= PARAGRAPH_SPACING_MM;
        }
        y
    }
}

// ---------- Header / metadata / sections / footer ----------

fn render_header(ctx: &mut RenderContext, data: &MeetingExportData) -> Result<(), String> {
    // Ensure space for title (wrapped, max 2 lines) + meta line + gap
    ctx.ensure_space(LINE_HEIGHT_TITLE * 2.0 + LINE_HEIGHT_BODY + 4.0);
    ctx.write_wrapped(&data.meeting_title, TITLE_SIZE_PT, FontWeight::Bold);
    ctx.write_line(
        &format!(
            "Generated by Personal Meetly • Template: {}",
            data.template_name
        ),
        META_SIZE_PT,
        FontWeight::Regular,
    );
    ctx.cursor_y -= 4.0;
    Ok(())
}

fn render_metadata(ctx: &mut RenderContext, data: &MeetingExportData) -> Result<(), String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("Meeting ID: {}", data.meeting_id));
    lines.push(format!("Date: {}", format_date_human(&data.created_at)));

    if let Some(duration) = &data.duration {
        if !duration.is_empty() {
            lines.push(format!("Duration: {duration}"));
        }
    }
    if let Some(attendees) = &data.attendees {
        let cleaned = attendees.trim();
        if !cleaned.is_empty() {
            lines.push(format!("Attendees: {cleaned}"));
        }
    }

    for line in lines {
        ctx.write_wrapped(&line, META_SIZE_PT, FontWeight::Regular);
    }
    Ok(())
}

fn render_divider(ctx: &mut RenderContext) -> Result<(), String> {
    ctx.cursor_y -= 1.0;
    let layer = ctx.layer_ref();
    let line = Line {
        points: vec![
            (Point::new(Mm(CONTENT_LEFT_MM), Mm(ctx.cursor_y)), false),
            (Point::new(Mm(CONTENT_RIGHT_MM), Mm(ctx.cursor_y)), false),
        ],
        is_closed: false,
        has_fill: false,
        has_stroke: true,
        is_clipping_path: false,
    };
    layer.set_outline_thickness(0.3);
    layer.add_shape(line);
    ctx.cursor_y -= 4.0;
    Ok(())
}

fn render_sections(ctx: &mut RenderContext, data: &MeetingExportData) -> Result<(), String> {
    for (idx, section) in data.sections.iter().enumerate() {
        if idx > 0 {
            ctx.cursor_y -= SECTION_SPACING_MM;
        }
        ctx.ensure_space(LINE_HEIGHT_HEADING * 2.0);
        ctx.write_line(&section.title, SECTION_HEADING_SIZE_PT, FontWeight::Bold);

        match section.format.as_str() {
            "list" => render_list(ctx, &section.content, section.item_format.as_deref()),
            "string" => render_string_section(ctx, &section.content),
            // Default to paragraph (also covers unknown future formats)
            _ => ctx.write_wrapped(&section.content, BODY_SIZE_PT, FontWeight::Regular),
        }
    }
    Ok(())
}

fn render_list(ctx: &mut RenderContext, content: &str, item_format: Option<&str>) {
    // Detect markdown pipe tables (lines starting with `|`). Render each
    // consecutive run of pipe-lines as a real grid (the grid renderer
    // falls back to the plain text-with-`│` renderer internally when the
    // table won't fit); render any non-pipe lines around/between tables
    // as regular prose so a lead-in like "See below:" isn't dropped.
    let any_pipe = content.lines().any(|l| l.trim_start().starts_with('|'));
    if any_pipe {
        render_list_segmented(ctx, content, item_format);
        return;
    }

    // Detect checkbox action items (`- [ ] task [[Owner]] Due: date`)
    // and synthesize a 3-col pipe table so the grid renderer handles
    // layout. This is a fallback for when the LLM ignores the
    // pipe-table `item_format` and emits Markdown checkboxes instead.
    if looks_like_checkbox_list(content) {
        let pipe_table = checkbox_list_to_pipe_table(content);
        // ponytail: pass item_format=None because the synthesized table
        // has 3 cols while the template's item_format may have a
        // different col count, which would force a fallback. Explicit
        // header is baked into the synthesized table instead. Ceiling:
        // synthesized header is English ("Task | Owner | Due") and
        // isn't localized to the section's language; revisit if needed.
        render_table_grid(ctx, &pipe_table, BODY_SIZE_PT, SECTION_HEADING_SIZE_PT, None);
        return;
    }

    // Detect structured bullet lists with field separators (|, :, or ,)
    // e.g., "- **Owner**: Ana | **Task**: Finish doc | **Due**: 2026-07-25"
    // or   "- Owner: Ana, Task: Finish doc, Due: 2026-07-25"
    let lines: Vec<&str> = content.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lines.len() >= 2 && looks_like_structured_list(&lines) {
        render_structured_list_as_table(ctx, &lines);
        return;
    }

    // Fallback: regular bullet list
    for line in lines {
        let bullet = format!("• {line}");
        ctx.write_wrapped(&bullet, BODY_SIZE_PT, FontWeight::Regular);
    }
}

/// Render content that contains at least one pipe table, splitting it
/// into ordered segments: consecutive `|`-lines become one table
/// segment (rendered via `render_table_grid`), consecutive non-`|`
/// lines become a prose segment (rendered via `write_wrapped`, one
/// line per call). Preserves prose lead-ins/trailings that were
/// previously dropped when a section mixed prose with a table.
fn render_list_segmented(ctx: &mut RenderContext, content: &str, item_format: Option<&str>) {
    let mut buf: Vec<&str> = Vec::new();
    let mut buf_kind: Option<SegmentKind> = None;
    for line in content.lines() {
        let kind = if line.trim_start().starts_with('|') { SegmentKind::Table } else { SegmentKind::Prose };
        if buf_kind.as_ref() != Some(&kind) {
            flush_segment(ctx, &buf, buf_kind.as_ref(), item_format);
            buf.clear();
            buf_kind = Some(kind);
        }
        buf.push(line);
    }
    flush_segment(ctx, &buf, buf_kind.as_ref(), item_format);
}

#[derive(PartialEq)]
enum SegmentKind { Prose, Table }

fn flush_segment(ctx: &mut RenderContext, lines: &[&str], kind: Option<&SegmentKind>, item_format: Option<&str>) {
    if lines.is_empty() {
        return;
    }
    let joined = lines.join("\n");
    match kind {
        Some(SegmentKind::Table) => {
            render_table_grid(ctx, &joined, BODY_SIZE_PT, SECTION_HEADING_SIZE_PT, item_format);
        }
        _ => {
            for line in joined.lines() {
                ctx.write_wrapped(line, BODY_SIZE_PT, FontWeight::Regular);
            }
        }
    }
}

/// Heuristic: check if lines look like structured key-value lists
/// with consistent separators (|, :, or comma-separated fields)
fn looks_like_structured_list(lines: &[&str]) -> bool {
    if lines.is_empty() {
        return false;
    }
    // Check first few lines for common field separator patterns
    let sample = lines.iter().take(3).collect::<Vec<_>>();
    let has_pipe = sample.iter().any(|l| l.contains('|'));
    let has_colon_fields = sample.iter().any(|l| l.matches(':').count() >= 2);
    let has_comma_fields = sample.iter().any(|l| l.matches(',').count() >= 2);
    has_pipe || has_colon_fields || has_comma_fields
}

/// Render a structured list (key-value pairs per line) as a table
fn render_structured_list_as_table(ctx: &mut RenderContext, lines: &[&str]) {
    // Try to extract headers from first line if it looks like a header
    let mut rows: Vec<Vec<String>> = Vec::new();

    for line in lines {
        let line = line.trim_start_matches(|c| c == '-' || c == '•' || c == '*').trim();
        if line.is_empty() {
            continue;
        }

        // Try pipe-separated first
        let cells: Vec<String> = if line.contains('|') {
            line.split('|').map(|c| c.trim().to_string()).collect()
        } else if line.matches(':').count() >= 2 {
            // Colon-separated: "Key: Value, Key: Value"
            line.split(',')
                .map(|part| {
                    let part = part.trim();
                    if let Some(idx) = part.find(':') {
                        format!("{}: {}", part[..idx].trim(), part[idx + 1..].trim())
                    } else {
                        part.to_string()
                    }
                })
                .collect()
        } else if line.matches(',').count() >= 2 {
            // Comma-separated: "Owner: Ana, Task: Finish, Due: Date"
            line.split(',').map(|c| c.trim().to_string()).collect()
        } else {
            vec![line.to_string()]
        };

        if cells.len() >= 2 {
            rows.push(cells);
        }
    }

    if rows.is_empty() {
        // Fallback to bullet list
        for line in lines {
            let bullet = format!("• {line}");
            ctx.write_wrapped(&bullet, BODY_SIZE_PT, FontWeight::Regular);
        }
        return;
    }

    // Render as table
    for row in rows {
        let joined = row.join("  │  ");
        ctx.write_wrapped(&joined, BODY_SIZE_PT, FontWeight::Regular);
    }
}

/// Heuristic: detect checkbox action-item lists (`- [ ] ...`).
/// Requires a strict majority of non-empty lines to start with `- [ ]`
/// or `- [x]` so a stray checkbox in a long bullet list does not
/// trigger table mode.
fn looks_like_checkbox_list(content: &str) -> bool {
    let non_empty: Vec<&str> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if non_empty.len() < 2 {
        return false;
    }
    let checkbox_count = non_empty
        .iter()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("- [ ]")
                || t.starts_with("- [x]")
                || t.starts_with("- [X]")
        })
        .count();
    checkbox_count * 2 >= non_empty.len() && checkbox_count >= 2
}

/// Convert `- [ ] task [[Owner]] Due: date` checkbox list into a
/// 3-col pipe table (`Task | Owner | Due`). Tolerant of missing
/// `[[Owner]]` or `Due:` tokens (empty cell). Lines that don't start
/// with a checkbox marker are skipped.
fn checkbox_list_to_pipe_table(content: &str) -> String {
    let mut out = String::from("| Task | Owner | Due |\n| --- | --- | --- |\n");
    for line in content.lines() {
        let t = line.trim();
        let body = if let Some(b) = t.trim_start().strip_prefix("- [ ]") {
            b.trim()
        } else if let Some(b) = t.trim_start().strip_prefix("- [x]") {
            b.trim()
        } else if let Some(b) = t.trim_start().strip_prefix("- [X]") {
            b.trim()
        } else {
            continue;
        };
        // extract `[[Owner]]`
        let mut owner = String::new();
        let mut rest = body.to_string();
        if let (Some(open), Some(close)) = (rest.find("[["), rest.find("]]")) {
            if open < close {
                owner = rest[open + 2..close].trim().to_string();
                rest = format!("{} {}", &rest[..open], &rest[close + 2..]);
            }
        }
        // extract `Due: <date>` (first whitespace-delimited token after `Due:`)
        let mut due = String::new();
        if let Some(idx) = rest.find("Due:") {
            let after = rest[idx + 4..].trim_start();
            let date = after.split_whitespace().next().unwrap_or("");
            if !date.is_empty() {
                due = date.to_string();
                let leading_ws = rest[idx + 4..].len() - after.len();
                let tail_start = idx + 4 + leading_ws + date.len();
                rest = format!("{} {}", &rest[..idx].trim(), &rest[tail_start..]);
            }
        }
        let task = rest.trim().trim_end_matches('-').trim().to_string();
        out.push_str(&format!("| {} | {} | {} |\n", task, owner, due));
    }
    out
}

fn render_markdown_table(ctx: &mut RenderContext, content: &str) {
    // Pull only the data rows, skipping the `|---|---|...` separator
    // and surrounding whitespace. Render each cell on its own line,
    // joined with "  │  " so the output remains readable in plain
    // PDF text without requiring true table layout.
    let mut rows: Vec<Vec<String>> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("|---") || trimmed.starts_with("| ---") {
            continue;
        }
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();
        rows.push(cells);
    }

    if rows.is_empty() {
        return;
    }

    for row in rows {
        let joined = row.join("  │  ");
        ctx.write_wrapped(&joined, BODY_SIZE_PT, FontWeight::Regular);
    }
}

/// Render a markdown pipe-table as a real grid: aligned columns, borders,
/// per-cell wrapping, row-level pagination, and header repetition on
/// continuation pages. Returns `true` if rendered as a grid, `false` if
/// the table fell back to the plain text-with-`│` renderer.
fn render_table_grid(
    ctx: &mut RenderContext,
    markdown: &str,
    body_font_size: f64,
    header_font_size: f64,
    item_format: Option<&str>,
) -> bool {
    // ---- Parse ----
    let all_lines: Vec<&str> = markdown
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if all_lines.is_empty() {
        return false;
    }

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut has_explicit_header = false;
    for line in &all_lines {
        if !line.starts_with('|') || !line.ends_with('|') {
            continue;
        }
        let inner = &line[1..line.len() - 1];
        let parts: Vec<&str> = inner.split('|').collect();
        let is_sep = !parts.is_empty()
            && parts
                .iter()
                .all(|p| !p.trim().is_empty() && p.trim().chars().all(|c| c == '-' || c == ':'));
        if is_sep {
            if !rows.is_empty() {
                has_explicit_header = true;
            }
            continue;
        }
        rows.push(parts.iter().map(|c| c.trim().to_string()).collect());
    }
    if rows.is_empty() {
        render_markdown_table(ctx, markdown);
        return false;
    }
    let num_cols = rows[0].len();
    if num_cols < 2 {
        render_markdown_table(ctx, markdown);
        return false;
    }
    if rows.iter().any(|r| r.len() != num_cols) {
        render_markdown_table(ctx, markdown);
        return false;
    }

    // ---- Header synthesis ----
    // Header cells are stripped of `**` bold markers (templates and the
    // LLM bold headers; the grid draws them bold already, so literal
    // asterisks would otherwise leak into the PDF).
    let (header, data_rows): (Vec<String>, Vec<Vec<String>>) = if has_explicit_header {
        let h: Vec<String> = rows
            .remove(0)
            .into_iter()
            .map(|c| c.trim_matches('*').trim().to_string())
            .collect();
        (h, rows)
    } else if let Some(fmt) = item_format {
        if !fmt.trim_start().starts_with('|') {
            render_markdown_table(ctx, markdown);
            return false;
        }
        let first_line = match fmt.lines().map(|l| l.trim()).find(|l| !l.is_empty()) {
            Some(fl) => fl,
            None => {
                render_markdown_table(ctx, markdown);
                return false;
            }
        };
        if !first_line.starts_with('|') || !first_line.ends_with('|') {
            render_markdown_table(ctx, markdown);
            return false;
        }
        let inner = &first_line[1..first_line.len() - 1];
        let parsed: Vec<String> = inner
            .split('|')
            .map(|c| c.trim().trim_matches('*').trim().to_string())
            .collect();
        if parsed.len() != num_cols {
            warn!(
                "item_format header has {} cols but table data has {}; falling back to plain table rendering",
                parsed.len(),
                num_cols
            );
            render_markdown_table(ctx, markdown);
            return false;
        }
        (parsed, rows)
    } else {
        let h: Vec<String> = (1..=num_cols).map(|i| format!("Col {i}")).collect();
        (h, rows)
    };

    // ---- Column widths ----
    const BORDER_BUDGET_MM: f64 = 0.5;
    let border_total_mm = (num_cols as f64 + 1.0) * BORDER_BUDGET_MM;
    let available_mm = CONTENT_WIDTH_MM - border_total_mm;
    if available_mm <= 0.0 {
        render_markdown_table(ctx, markdown);
        return false;
    }

    let measure_longest_word = |cell: &str| -> usize {
        cell.split_whitespace().map(|w| w.chars().count()).max().unwrap_or(0)
    };

    let mut col_min_chars: Vec<usize> = vec![0; num_cols];
    for (i, cell) in header.iter().enumerate() {
        let m = measure_longest_word(cell);
        if m > col_min_chars[i] {
            col_min_chars[i] = m;
        }
    }
    for row in &data_rows {
        for (i, cell) in row.iter().enumerate() {
            let m = measure_longest_word(cell);
            if m > col_min_chars[i] {
                col_min_chars[i] = m;
            }
        }
    }
    let total_min_chars: usize = col_min_chars.iter().sum();
    if total_min_chars == 0 {
        render_markdown_table(ctx, markdown);
        return false;
    }

    let available_chars = approx_chars_per_line(body_font_size, available_mm) as f64;
    let mm_per_char = available_mm / available_chars;

    let mut col_chars: Vec<f64> = col_min_chars.iter().map(|&c| c as f64).collect();
    let total_chars: f64 = col_chars.iter().sum();
    if total_chars > available_chars {
        // Scale down proportionally so each column's longest word still fits.
        let scale = available_chars / total_chars;
        for c in &mut col_chars {
            *c *= scale;
        }
    } else {
        // Distribute leftover equally so the table fills the printable width.
        let per_col = (available_chars - total_chars) / num_cols as f64;
        for c in &mut col_chars {
            *c += per_col;
        }
    }

    let mut col_width_mm: Vec<f64> = col_chars.iter().map(|c| c * mm_per_char).collect();
    for w in &mut col_width_mm {
        if *w < TABLE_MIN_COL_WIDTH_MM {
            *w = TABLE_MIN_COL_WIDTH_MM;
        }
    }
    let total_table_width: f64 = col_width_mm.iter().sum::<f64>() + border_total_mm;
    if total_table_width > CONTENT_WIDTH_MM {
        // ponytail: ceiling — table with many narrow columns or one oversized
        // header + min-col-width floor exceeds the printable area. Fall back
        // to the plain text-with-`│` renderer. Upgrade path: scale by
        // min-font-size rather than min-col-width, or transpose the table.
        render_markdown_table(ctx, markdown);
        return false;
    }

    // ponytail: a single row taller than one page would paginate mid-row —
    // borders drawn on the old page, overflow text landing borderless on
    // the new page, and a stale cursor for the rest of the table. Fall
    // back to the line-by-line plain renderer instead. Upgrade path:
    // split oversized cells across pages with clipped borders.
    let page_content_mm = CONTENT_TOP_MM - CONTENT_BOTTOM_MM;
    let header_height = compute_row_height(&header, &col_width_mm, header_font_size, TABLE_HEADER_PADDING_MM);
    if header_height > page_content_mm
        || data_rows.iter().any(|r| {
            compute_row_height(r, &col_width_mm, body_font_size, TABLE_CELL_PADDING_MM)
                > page_content_mm
        })
    {
        render_markdown_table(ctx, markdown);
        return false;
    }

    // Compute x positions for each column's left edge.
    let mut col_left: Vec<f64> = Vec::with_capacity(num_cols);
    let mut x = CONTENT_LEFT_MM + BORDER_BUDGET_MM;
    for i in 0..num_cols {
        col_left.push(x);
        x += col_width_mm[i] + BORDER_BUDGET_MM;
    }
    let table_right = col_left[num_cols - 1] + col_width_mm[num_cols - 1] + BORDER_BUDGET_MM;

    // ---- "First row only" guard ----
    // Don't start the table on a page where only the header + at most 1
    // data row would fit: the next-row check would then push to a fresh
    // page and re-emit the header, producing visually a "doubled header"
    // (header at the bottom of page N, header again at the top of page
    // N+1, with a single lonely row squashed between them on page N).
    // We require room on the CURRENT page for header + the FIRST data
    // row + the SECOND data row (all measured at their actual heights).
    // If not present, move the table start to a fresh page. The pre-row
    // pagination loop (below) keeps doing the right thing once we've
    // committed to starting the table.
    //
    // Single-row tables are NOT guarded — if only the header+row fits
    // on the cramped page, the row is rendered there (the next section
    // continues on the next page). The "doubled header" artifact needs
    // ≥2 rows because the trigger is the per-row pagination before N+1
    // triggering header-repeat.
    let header_h = compute_row_height(&header, &col_width_mm, header_font_size, TABLE_HEADER_PADDING_MM);
    let want_room = if data_rows.len() >= 2 {
        let first_row_h = compute_row_height(
            &data_rows[0],
            &col_width_mm,
            body_font_size,
            TABLE_CELL_PADDING_MM,
        );
        let second_row_h = compute_row_height(
            &data_rows[1],
            &col_width_mm,
            body_font_size,
            TABLE_CELL_PADDING_MM,
        );
        header_h + first_row_h + second_row_h
    } else {
        // 0 or 1 data rows: nothing to guard against (no second row to
        // trigger header repeat); use 0 → guard is no-op.
        0.0
    };
    if want_room > 0.0 && ctx.cursor_y - want_room < CONTENT_BOTTOM_MM {
        // Page would orphan the header + first row alone. Move the table
        // start to a fresh page so the header isn't visually doubled on
        // overflow. Caveat: if a fresh page *also* can't fit header +
        // first + second row (rows nearly fill a page), we accept the
        // smaller layout — fall through and let the per-row loop handle
        // it. The page-content sanity gate at the top already excludes
        // the catastrophic "header alone taller than a page" or "any
        // one row taller than a page" cases.
        let fresh_room = CONTENT_TOP_MM - CONTENT_BOTTOM_MM;
        if fresh_room >= want_room {
            ctx.new_page();
        }
    }

    // ---- Render header first ----
    let header_h = compute_row_height(
        &header,
        &col_width_mm,
        header_font_size,
        TABLE_HEADER_PADDING_MM,
    );
    ctx.ensure_space(header_h);
    draw_table_row(
        ctx,
        &header,
        &col_left,
        &col_width_mm,
        table_right,
        header_font_size,
        FontWeight::Bold,
        TABLE_HEADER_PADDING_MM,
        header_h,
    );

    // ---- Render data rows with pagination + header repeat ----
    for data_row in &data_rows {
        let row_height = compute_row_height(data_row, &col_width_mm, body_font_size, TABLE_CELL_PADDING_MM);
        if ctx.cursor_y - row_height < CONTENT_BOTTOM_MM {
            ctx.new_page();
            if TABLE_HEADER_REPEAT {
                draw_table_row(
                    ctx,
                    &header,
                    &col_left,
                    &col_width_mm,
                    table_right,
                    header_font_size,
                    FontWeight::Bold,
                    TABLE_HEADER_PADDING_MM,
                    header_h,
                );
            }
        }
        draw_table_row(
            ctx,
            data_row,
            &col_left,
            &col_width_mm,
            table_right,
            body_font_size,
            FontWeight::Regular,
            TABLE_CELL_PADDING_MM,
            row_height,
        );
    }

    true
}

/// Compute the height (mm) of a table row by pre-measuring the wrapped
/// line count of each cell at the given column widths. MUST mirror the
/// actual draw path (`_write_wrapped_impl_opts(allow_paginate=false)` in
/// `draw_table_row`) exactly: per-paragraph `wrap_to_width` (not
/// whole-cell), identical `line_height` selection by `font_size`, the
/// inter-paragraph `PARAGRAPH_SPACING_MM` gap, and the first/last-line
/// ascent/descent budget so glyphs stay inside the cell borders.
///
/// Invariant: `bottom_y = cursor_y - compute_row_height(...)` is the
/// lowest line-of-text position the draw path can reach. The pre-row
/// pagination check (`cursor_y - row_height >= CONTENT_BOTTOM_MM`) and
/// the mid-cell silent-drop guard (`y - descent < CONTENT_BOTTOM_MM`)
/// BOTH depend on this staying aligned with the draw path. If they ever
/// diverge, the symptom is either an orphan header (pre-check too
/// pessimistic) or a silently-dropped cell line (pre-check too
/// optimistic). One runnable check lives in
/// `draw_table_row_does_not_paginate_mid_cell_when_row_fits`.
fn compute_row_height(
    row: &[String],
    col_width_mm: &[f64],
    font_size: f64,
    v_padding: f64,
) -> f64 {
    // ponytail: 0.8em / 0.2em ascent/descent split matches the `cell_y`
    // heuristic in `draw_table_row`; font metrics would be tighter but
    // the heuristic keeps this free-function self-contained.
    let ascent = font_size * 0.3528 * 0.8;
    let descent = font_size * 0.3528 * 0.2;
    let line_height = if font_size >= TITLE_SIZE_PT {
        LINE_HEIGHT_TITLE
    } else if font_size >= SECTION_HEADING_SIZE_PT {
        LINE_HEIGHT_HEADING
    } else {
        LINE_HEIGHT_BODY
    };

    let mut max_text_height = 0.0_f64; // height from first baseline to last baseline (inclusive of last line)
    for (i, cell) in row.iter().enumerate() {
        let text_width = (col_width_mm[i] - 2.0 * v_padding).max(1.0);
        let paragraphs = split_paragraphs(cell);
        let mut sum_lines: usize = 0;
        let mut para_count: usize = 0;
        for p in &paragraphs {
            sum_lines += wrap_to_width(p, text_width, font_size).len();
            para_count += 1;
        }
        if sum_lines == 0 {
            sum_lines = 1;
        }
        // first baseline at cell_y; each subsequent line drops `line_height`;
        // each paragraph boundary adds one extra `PARAGRAPH_SPACING_MM`.
        let span = (sum_lines - 1) as f64 * line_height
            + para_count.saturating_sub(1) as f64 * PARAGRAPH_SPACING_MM;
        if span > max_text_height {
            max_text_height = span;
        }
    }

    // Total row height = top v_padding (above first glyph) +
    // ascent (first glyph above first baseline) + text span +
    // descent (last glyph below last baseline) + bottom v_padding.
    2.0 * v_padding + ascent + max_text_height + descent
}

/// Render a single row of the table at the current cursor position:
/// top + bottom borders, vertical separators, and per-cell text.
/// Advances `cursor_y` by the row height. Caller is responsible for
/// page-break checks.
///
/// `row_height` is pre-computed by the caller (the render loop already
/// measured it for the pagination check); passing it in avoids a
/// redundant `compute_row_height` call per row, which matters for
/// large tables where each cell wrap is non-trivial. The caller and
/// callee must use the same column widths / font size / v_padding.
fn draw_table_row(
    ctx: &mut RenderContext,
    row: &[String],
    col_left: &[f64],
    col_width_mm: &[f64],
    table_right: f64,
    font_size: f64,
    weight: FontWeight,
    v_padding: f64,
    row_height: f64,
) {
    let num_cols = row.len();

    let layer = ctx.layer_ref();
    layer.set_outline_thickness(TABLE_BORDER_WIDTH_PT);

    // Top border
    let top_line = Line {
        points: vec![
            (Point::new(Mm(CONTENT_LEFT_MM), Mm(ctx.cursor_y)), false),
            (Point::new(Mm(table_right), Mm(ctx.cursor_y)), false),
        ],
        is_closed: false,
        has_fill: false,
        has_stroke: true,
        is_clipping_path: false,
    };
    layer.add_shape(top_line);

    // Bottom border
    let bottom_y = ctx.cursor_y - row_height;
    let bottom_line = Line {
        points: vec![
            (Point::new(Mm(CONTENT_LEFT_MM), Mm(bottom_y)), false),
            (Point::new(Mm(table_right), Mm(bottom_y)), false),
        ],
        is_closed: false,
        has_fill: false,
        has_stroke: true,
        is_clipping_path: false,
    };
    layer.add_shape(bottom_line);

    // Vertical borders
    for i in 0..=num_cols {
        let vx = if i == 0 {
            CONTENT_LEFT_MM
        } else if i == num_cols {
            table_right
        } else {
            col_left[i]
        };
        let vline = Line {
            points: vec![
                (Point::new(Mm(vx), Mm(ctx.cursor_y)), false),
                (Point::new(Mm(vx), Mm(bottom_y)), false),
            ],
            is_closed: false,
            has_fill: false,
            has_stroke: true,
            is_clipping_path: false,
        };
        layer.add_shape(vline);
    }

    // Cell text
    for (i, cell) in row.iter().enumerate() {
        let cell_x = col_left[i] + TABLE_CELL_PADDING_MM;
        // ponytail: ~0.8em ≈ font ascent; keeps glyphs below the top border.
        // printpdf places the baseline at y, so subtract ascent before v_padding.
        let cell_y = ctx.cursor_y - v_padding - font_size * 0.3528 * 0.8;
        let cell_text_width = (col_width_mm[i] - 2.0 * TABLE_CELL_PADDING_MM).max(1.0);
        // Non-paginating variant: the row's pre-check (caller) already
        // verified that the whole row fits on this page, and
        // `compute_row_height` uses the same `wrap_to_width` as the draw
        // path, so every line stays within the row's bottom border.
        // Paginating mid-cell would split the row across pages with
        // text on the new page but cell borders on the old one.
        let _ = ctx._write_wrapped_impl_opts(
            cell,
            font_size,
            weight,
            cell_x,
            cell_y,
            cell_text_width,
            false,
        );
    }

    ctx.cursor_y = bottom_y;
}

fn render_string_section(ctx: &mut RenderContext, content: &str) {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        ctx.write_line("(no content)", BODY_SIZE_PT, FontWeight::Regular);
        return;
    }
    ctx.write_wrapped(trimmed, BODY_SIZE_PT, FontWeight::Regular);
}

/// Materialise a `FontHandle` into a real `IndirectFontRef` for the
/// current document. Built-in fonts are looked up on demand; the
/// resulting `IndirectFontRef` is cheap to clone, so it can be held
/// across borrows.
fn resolve_font(ctx: &RenderContext, weight: FontWeight) -> Result<IndirectFontRef, String> {
    let handle = match weight {
        FontWeight::Bold => ctx.fonts.bold(),
        FontWeight::Regular => ctx.fonts.regular(),
    };
    match handle {
        FontHandle::External(f) => Ok(f),
        FontHandle::Builtin(b) => ctx
            .doc
            .add_builtin_font(b)
            .map_err(|e| format!("Failed to load builtin font {:?}: {}", b, e)),
    }
}

// ---------- Font loading ----------

/// Candidate file names for the regular weight, in priority order.
const REGULAR_FONT_CANDIDATES: &[&str] = &[
    "DejaVuSans.ttf",
    "NotoSans-Regular.ttf",
    "Arial.ttf",
    "LiberationSans-Regular.ttf",
    "Helvetica.ttf",
];

/// Candidate file names for the bold weight, in priority order.
const BOLD_FONT_CANDIDATES: &[&str] = &[
    "DejaVuSans-Bold.ttf",
    "NotoSans-Bold.ttf",
    "Arial-Bold.ttf",
    "LiberationSans-Bold.ttf",
];

fn load_unicode_font(doc: &PdfDocumentReference, weight: FontWeight) -> Result<FontHandle, String> {
    let candidates: &[&str] = match weight {
        FontWeight::Regular => REGULAR_FONT_CANDIDATES,
        FontWeight::Bold => BOLD_FONT_CANDIDATES,
    };

    if let Some(path) = find_font_file(candidates) {
        let open_result = File::open(&path).map(BufReader::new);
        match open_result {
            Ok(reader) => match doc.add_external_font(reader) {
                Ok(font) => {
                    info!(
                        "Embedded {} font from {} for PDF export",
                        match weight {
                            FontWeight::Regular => "regular",
                            FontWeight::Bold => "bold",
                        },
                        path.display()
                    );
                    return Ok(FontHandle::External(font));
                }
                Err(e) => {
                    warn!(
                        "Found font file {} but failed to embed it: {}. Falling back to builtin.",
                        path.display(),
                        e
                    );
                }
            },
            Err(e) => {
                warn!(
                    "Found font path {} but could not open it: {}. Falling back to builtin.",
                    path.display(),
                    e
                );
            }
        }
    }

    let builtin = match weight {
        FontWeight::Regular => BuiltinFont::Helvetica,
        FontWeight::Bold => BuiltinFont::HelveticaBold,
    };
    warn!(
        "No Unicode font found; falling back to {:?}. PT-BR characters may be lost.",
        builtin
    );
    Ok(FontHandle::Builtin(builtin))
}

fn find_font_file(candidates: &[&str]) -> Option<PathBuf> {
    let roots = font_search_roots();
    for root in roots {
        for candidate in candidates {
            let p = root.join(candidate);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn font_search_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Ok(override_dir) = std::env::var("MEETLY_PDF_FONT_DIR") {
        roots.push(PathBuf::from(override_dir));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.join("fonts"));
            roots.push(parent.join("../Resources/fonts")); // macOS bundle
            roots.push(parent.join("../templates/fonts"));
            roots.push(parent.join("../Resources/templates/fonts")); // Tauri macOS bundle
            roots.push(parent.join("resources/templates/fonts"));    // Tauri Windows/Linux bundle
            roots.push(parent.join("../resources/templates/fonts")); // Tauri Linux AppImage
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("templates/fonts"));
    }

    for system_path in platform_font_dirs() {
        roots.push(system_path);
    }

    roots
}

fn platform_font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        }
        dirs.push(PathBuf::from("C:/Windows/Fonts"));
    }

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/share/fonts/truetype/dejavu"));
        dirs.push(PathBuf::from("/usr/share/fonts/truetype/noto"));
        dirs.push(PathBuf::from("/usr/share/fonts/dejavu"));
        dirs.push(PathBuf::from("/usr/share/fonts/TTF"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/fonts"));
        }
    }

    dirs
}

// ---------- Font metrics (real glyph widths) ----------
//
// ponytail: parses DejaVuSans.ttf (and the bold variant) directly from
// disk to extract `unitsPerEm`, the `hmtx` advance widths and a `cmap`
// format-4 subtable. We hand-roll the TTF parsing rather than pulling
// in a font crate (~80 LOC vs a new dependency); the supported font is
// fixed and the TTF table layout for the fields we need is stable.
// Ceiling: only `cmap` subtable format 4 (BMP) is handled; supplementary
// plane characters (emoji etc.) fall back to a default advance, which
// may slightly mis-wrap — fine for meeting notes. Also: only one font
// family (DejaVuSans) is parsed; if the prod font is swapped, the
// metrics module must be re-validated.

use std::sync::OnceLock;

struct FontMetrics {
    units_per_em: u16,
    /// Advance width (font units) per glyph index.
    advances: Vec<u16>,
    /// `cmap` format-4 segment table.
    seg_end: Vec<u16>,
    seg_start: Vec<u16>,
    seg_delta: Vec<i16>,
    seg_offset: Vec<u16>,
    glyph_id_array: Vec<u16>,
}

static FONT_METRICS: OnceLock<Option<FontMetrics>> = OnceLock::new();

fn font_metrics() -> Option<&'static FontMetrics> {
    FONT_METRICS
        .get_or_init(|| {
            // Try the same candidate font files the embedder loads.
            let path = find_font_file(REGULAR_FONT_CANDIDATES)?;
            let bytes = std::fs::read(&path).ok()?;
            let m = parse_ttf_metrics(&bytes)?;
            info!(
                "Loaded PDF font metrics from {} (UPM={}, {} glyphs, {} cmap segments)",
                path.display(),
                m.units_per_em,
                m.advances.len(),
                m.seg_end.len()
            );
            Some(m)
        })
        .as_ref()
}

// ponytail: separate metrics for the bold weight. `write_line_at_segments`
// measures x-advance per segment; using Regular advances for a Bold segment
// undercounts the rendered width (DejaVuSans-Bold glyphs are slightly wider
// than Regular), so the following Regular segment would start too far left
// and overlap the Bold tail's last characters horizontally. Loading bold
// metrics lets us measure Bold segments with their real advances.
// Ceiling: if no Bold TTF is found OR `parse_ttf_metrics` fails, we fall
// back to the Regular metrics (the previous, buggy behaviour — overlap
// returns). Upgrade path: a unit test that asserts bold metrics loaded
// whenever regular metrics did and BOLD_FONT_CANDIDATES resolve to a file
// of the same family (already enforced by `load_unicode_font` picking the
// matching bold file by convention, not a hard invariant here).
static BOLD_FONT_METRICS: OnceLock<Option<FontMetrics>> = OnceLock::new();

fn bold_font_metrics() -> Option<&'static FontMetrics> {
    BOLD_FONT_METRICS
        .get_or_init(|| {
            let path = find_font_file(BOLD_FONT_CANDIDATES)?;
            let bytes = std::fs::read(&path).ok()?;
            let m = parse_ttf_metrics(&bytes)?;
            info!(
                "Loaded PDF bold font metrics from {} (UPM={}, {} glyphs, {} cmap segments)",
                path.display(),
                m.units_per_em,
                m.advances.len(),
                m.seg_end.len()
            );
            Some(m)
        })
        .as_ref()
}

/// Minimal TTF parser for the tables we need: `head`, `hhea`, `hmtx`, `cmap` (fmt 4).
fn parse_ttf_metrics(bytes: &[u8]) -> Option<FontMetrics> {
    // SFNT header: u32 magic, u16 numTables, u16 searchRange, u16 entrySelector, u16 rangeShift
    if bytes.len() < 12 {
        return None;
    }
    let num_tables = u16_at(bytes, 4)?;
    let mut table_offsets: std::collections::HashMap<[u8; 4], (usize, usize)> =
        std::collections::HashMap::new();
    for i in 0..num_tables {
        let off = 12 + (i as usize) * 16;
        if off + 16 > bytes.len() {
            return None;
        }
        let tag = [bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]];
        let t_off = u32_at(bytes, off + 8)? as usize;
        let t_len = u32_at(bytes, off + 12)? as usize;
        table_offsets.insert(tag, (t_off, t_len));
    }

    let (head_off, head_len) = *table_offsets.get(b"head")?;
    if head_off + 54 > bytes.len() || head_len < 54 {
        return None;
    }
    let units_per_em = u16_at(bytes, head_off + 18)?;

    let (hhea_off, hhea_len) = *table_offsets.get(b"hhea")?;
    if hhea_off + 36 > bytes.len() || hhea_len < 36 {
        return None;
    }
    let num_h_metrics = u16_at(bytes, hhea_off + 34)? as usize;

    let (hmtx_off, hmtx_len) = *table_offsets.get(b"hmtx")?;
    if hmtx_off + num_h_metrics * 4 > bytes.len() || hmtx_len < num_h_metrics * 4 {
        return None;
    }
    // `numberOfHMetrics` long entries; remaining glyphs use the last advance.
    let mut advances: Vec<u16> = Vec::with_capacity(num_h_metrics);
    let mut last_advance: u16 = 0;
    for i in 0..num_h_metrics {
        let off = hmtx_off + i * 4;
        let aw = u16_at(bytes, off)?;
        advances.push(aw);
        last_advance = aw;
    }
    // Total glyph count comes from `maxp` (offset 4: u16 numGlyphs).
    let total_glyphs = table_offsets
        .get(b"maxp")
        .and_then(|&(o, l)| if o + 4 <= bytes.len() && l >= 4 { u16_at(bytes, o + 4) } else { None })
        .map(|n| n as usize)
        .unwrap_or(num_h_metrics);
    if total_glyphs > num_h_metrics {
        advances.resize(total_glyphs, last_advance);
    }

    // Parse cmap: find a format-4 subtable.
    let (cmap_off, cmap_len) = *table_offsets.get(b"cmap")?;
    if cmap_off + 4 > bytes.len() || cmap_len < 4 {
        return None;
    }
    let num_subtables = u16_at(bytes, cmap_off + 2)? as usize;
    let mut fmt4_off: Option<usize> = None;
    for i in 0..num_subtables {
        let rec = cmap_off + 4 + i * 8;
        if rec + 8 > bytes.len() {
            break;
        }
        // platformID u16, encodingID u16, subtable offset u32 (from cmap start)
        let platform = u16_at(bytes, rec)?;
        let encoding = u16_at(bytes, rec + 2)?;
        let sub_rel = u32_at(bytes, rec + 4)? as usize;
        let sub = cmap_off + sub_rel;
        if sub + 2 > bytes.len() {
            continue;
        }
        let format = u16_at(bytes, sub)?;
        if format == 4 && platform == 3 && (encoding == 1 || encoding == 0) {
            fmt4_off = Some(sub);
            break;
        }
        // Fallback: also accept platform 0 (Unicode) format 4.
        if format == 4 && platform == 0 && fmt4_off.is_none() {
            fmt4_off = Some(sub);
        }
    }
    let sub = fmt4_off?;
    // Format 4 layout:
    //   u16 format, u16 length, u16 language,
    //   u16 segCountX2, u16 searchRange, u16 entrySelector, u16 rangeShift,
    //   u16 endCode[segCount], u16 reservedPad, u16 startCode[segCount],
    //   i16 idDelta[segCount], u16 idRangeOffset[segCount],
    //   u16 glyphIdArray[...]
    if sub + 14 > bytes.len() {
        return None;
    }
    let seg_count = (u16_at(bytes, sub + 6)? as usize) / 2;
    let after_header = sub + 14;
    let end_base = after_header;
    let pad_base = end_base + seg_count * 2;
    let start_base = pad_base + 2;
    let delta_base = start_base + seg_count * 2;
    let range_base = delta_base + seg_count * 2;
    let glyph_array_base = range_base + seg_count * 2;
    if glyph_array_base > bytes.len() {
        return None;
    }
    let mut seg_end = Vec::with_capacity(seg_count);
    let mut seg_start = Vec::with_capacity(seg_count);
    let mut seg_delta = Vec::with_capacity(seg_count);
    let mut seg_offset = Vec::with_capacity(seg_count);
    for i in 0..seg_count {
        seg_end.push(u16_at(bytes, end_base + i * 2)?);
        seg_start.push(u16_at(bytes, start_base + i * 2)?);
        seg_delta.push(i16_at(bytes, delta_base + i * 2)?);
        seg_offset.push(u16_at(bytes, range_base + i * 2)?);
    }
    // The remainder of the subtable is glyphIdArray (u16 each).
    let sub_len = u16_at(bytes, sub + 2)? as usize;
    let sub_end = sub + sub_len;
    let mut glyph_id_array = Vec::new();
    let mut p = glyph_array_base;
    while p + 2 <= sub_end && p + 2 <= bytes.len() {
        glyph_id_array.push(u16_at(bytes, p)?);
        p += 2;
    }

    Some(FontMetrics {
        units_per_em,
        advances,
        seg_end,
        seg_start,
        seg_delta,
        seg_offset,
        glyph_id_array,
    })
}

impl FontMetrics {
    /// Map a Unicode scalar to its glyph index, or None if unsupported.
    fn glyph_index(&self, c: u32) -> Option<u16> {
        let cp = u16::try_from(c).ok()?;
        // Linear scan over segments (typically <300 in DejaVuSans).
        for (i, &end) in self.seg_end.iter().enumerate() {
            if cp > end {
                continue;
            }
            let start = self.seg_start[i];
            if cp < start {
                return None;
            }
            let delta = self.seg_delta[i];
            let offset = self.seg_offset[i];
            if offset == 0 {
                return Some(((cp as i32 + delta as i32) & 0xFFFF) as u16);
            }
            // idRangeOffset logic: pointer arithmetic into glyph_id_array.
            // C-level formula: ptr = &idRangeOffset[i] + offset + (cp - start),
            // measured in u16 units; subtracting the table-end offset
            // (seg_count - i slots from slot i to one-past-end) gives the
            // array index into glyph_id_array.
            let idx = (offset as usize / 2)
                + (cp as usize - start as usize)
                .saturating_sub(self.seg_end.len() - i);
            if idx >= self.glyph_id_array.len() {
                return None;
            }
            let gid = self.glyph_id_array[idx];
            if gid == 0 {
                return None;
            }
            return Some(((gid as i32 + delta as i32) & 0xFFFF) as u16);
        }
        None
    }

    /// Advance width (font units) for a Unicode char. Falls back to a
    /// typical proportional advance (0.5 × UPM = 1024 for DejaVuSans,
    /// whose default advance is ~600/2048) when the glyph is missing —
    /// keeps wrapping sane for unsupported codepoints.
    fn char_advance_units(&self, c: char) -> u16 {
        if let Some(gid) = self.glyph_index(c as u32) {
            let g = gid as usize;
            if g < self.advances.len() {
                return self.advances[g];
            }
        }
        // Common average; DejaVuSans default advance is ~600/2048.
        (self.units_per_em as f64 * 0.5) as u16
    }

    /// Width of `text` in millimeters at `size_pt` (real glyph advances).
    fn text_width_mm(&self, text: &str, size_pt: f64) -> f64 {
        let mut units: f64 = 0.0;
        for c in text.chars() {
            let adv = self.char_advance_units(c) as f64;
            units += adv;
        }
        // pt = size_pt * adv / units_per_em; mm = pt * 0.3528.
        units * size_pt * 0.3528 / (self.units_per_em as f64)
    }
}

#[inline]
fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    if off + 2 > b.len() {
        None
    } else {
        Some(u16::from_be_bytes([b[off], b[off + 1]]))
    }
}

#[inline]
fn i16_at(b: &[u8], off: usize) -> Option<i16> {
    // TTF stores i16 (e.g. `loca` deltas / `idDelta`) as the
    // big-endian bit pattern of an unsigned u16 — `as i16` reinterprets
    // the two's-complement value correctly. Not a range-checked u16→i16
    // conversion.
    u16_at(b, off).map(|v| v as i16)
}

#[inline]
fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    if off + 4 > b.len() {
        None
    } else {
        Some(u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]))
    }
}

/// Greedy word-wrap into lines that fit `max_width_mm` at `size_pt`,
/// using REAL glyph advance widths from the bundled font. Hard-breaks
/// words whose own width exceeds `max_width_mm` so no glyph is ever
/// wider than the line. Falls back to the coarse `wrap_paragraph` if
/// real metrics aren't available (e.g. builtin Helvetica fallback).
fn wrap_to_width(text: &str, max_width_mm: f64, size_pt: f64) -> Vec<String> {
    if let Some(m) = font_metrics() {
        wrap_to_width_real(m, text, max_width_mm, size_pt)
    } else {
        // ponytail: no metrics — fall back to the glyph-count heuristic so
        // the renderer still produces SOMETHING rather than panicking.
        wrap_paragraph(text, approx_chars_per_line(size_pt, max_width_mm))
    }
}

/// Split a line of text into `(weight, text)` segments by inline markdown
/// `**bold**` pairs. An unmatched/odd `**` is treated as literal text.
/// Returns a single Regular segment when the line has no inline bold.
///
/// ponytail: ceiling — wrap-width is measured without bold markers, but
/// `DejaVuSans-Bold` glyphs are slightly wider than Regular, so a line
/// with a long bold run may render a few percent wider than the column.
/// Acceptable for meeting notes (short bold labels + numbers); upgrade
/// path is per-segment shaping with the matching `Bold` metrics.
fn split_bold_segments(line: &str, base: FontWeight) -> Vec<(FontWeight, &str)> {
    if !line.contains("**") {
        return vec![(base, line)];
    }
    let bytes = line.as_bytes();
    let mut out: Vec<(FontWeight, &str)> = Vec::new();
    let mut seg_start = 0usize;
    let mut i = 0usize;
    let mut in_bold = false;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            // Flush pending segment.
            if i > seg_start {
                let weight = if in_bold { FontWeight::Bold } else { base };
                out.push((weight, &line[seg_start..i]));
            }
            // Toggle bold state and skip the two-asterisk delimiter.
            in_bold = !in_bold;
            i += 2;
            seg_start = i;
        } else {
            i += 1;
        }
    }
    // Trailing text after the last `**`.
    if seg_start < line.len() {
        let weight = if in_bold { FontWeight::Bold } else { base };
        out.push((weight, &line[seg_start..]));
    }
    // Odd-numbered `**` => trailing `in_bold == true`. Treat the dangling
    // text as Regular (literal asterisks would have been flushed earlier as
    // part of the visible text if they were unbalanced at the source).
    if in_bold {
        if let Some(last) = out.last_mut() {
            last.0 = base;
        }
    }
    out
}

fn wrap_to_width_real(
    m: &FontMetrics,
    text: &str,
    max_width_mm: f64,
    size_pt: f64,
) -> Vec<String> {
    let space_w = m.text_width_mm(" ", size_pt);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0.0_f64;

    for word in text.split_whitespace() {
        let word_w = m.text_width_mm(word, size_pt);
        // Hard-break a word that alone is wider than the line.
        if word_w > max_width_mm {
            // Flush whatever we have on the current line first.
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_w = 0.0;
            }
            // Hard-break the word char-by-char so each fragment fits.
            let mut chunk = String::new();
            let mut chunk_w = 0.0;
            for c in word.chars() {
                let cw = m.text_width_mm(&c.to_string(), size_pt);
                if chunk_w + cw > max_width_mm && !chunk.is_empty() {
                    lines.push(std::mem::take(&mut chunk));
                    chunk_w = 0.0;
                }
                chunk.push(c);
                chunk_w += cw;
            }
            if !chunk.is_empty() {
                current = chunk;
                current_w = chunk_w;
            }
            continue;
        }

        let added_w = if current.is_empty() { word_w } else { current_w + space_w + word_w };
        if added_w > max_width_mm {
            // Wrap: push current line, start new one with this word.
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
            current_w = word_w;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_w += space_w;
            }
            current.push_str(word);
            current_w += word_w;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

// ---------- Text shaping helpers ----------

fn split_paragraphs(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|p| p.trim_end_matches('\r').to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Greedy word-wrap into lines of (approximately) `max_chars` glyphs.
fn wrap_paragraph(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        // Hard-break words longer than the line width.
        if word.chars().count() > max_chars {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let mut chunk = String::new();
            for ch in word.chars() {
                if chunk.chars().count() >= max_chars {
                    lines.push(std::mem::take(&mut chunk));
                }
                chunk.push(ch);
            }
            if !chunk.is_empty() {
                current = chunk;
            }
            continue;
        }

        let tentative = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if tentative.chars().count() > max_chars {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        } else {
            current = tentative;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Heuristic: average glyph width ≈ 0.5 × font size in points.
/// 1 pt ≈ 0.3528 mm, so `chars_per_line ≈ width_mm / (size_pt × 0.1764)`.
fn approx_chars_per_line(size_pt: f64, width_mm: f64) -> usize {
    let chars = (width_mm / (size_pt * 0.18)).floor() as usize;
    chars.max(10)
}

fn format_date_human(iso: &str) -> String {
    // Best-effort human-friendly date; we deliberately don't pull in
    // `chrono` parsing here to keep this module self-contained.
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(iso) {
        return parsed
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%d %H:%M UTC")
            .to_string();
    }
    iso.to_string()
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_data() -> MeetingExportData {
        MeetingExportData {
            meeting_id: "abc-123".into(),
            meeting_title: "Reunião semanal — Planejamento Q3".into(),
            created_at: "2026-06-29T14:00:00Z".into(),
            duration: Some("00:42:13".into()),
            attendees: Some("Ana, Bruno, Carla".into()),
            template_name: "Standard Meeting Notes".into(),
            sections: vec![
                SectionContent {
                    title: "Summary".into(),
                    format: "paragraph".into(),
                    content: "Discutimos o roadmap do Q3 e alinhamos as entregas principais."
                        .into(),
                    item_format: None,
                },
                SectionContent {
                    title: "Action Items".into(),
                    format: "list".into(),
                    content: "Ana finaliza o documento de escopo\nBruno revisa orçamento".into(),
                    item_format: None,
                },
            ],
        }
    }

    #[test]
    fn wraps_short_paragraphs_into_single_line() {
        let lines = wrap_paragraph("hello world", 80);
        assert_eq!(lines, vec!["hello world".to_string()]);
    }

    #[test]
    fn wraps_long_paragraphs_on_word_boundaries() {
        let text = "the quick brown fox jumps over the lazy dog";
        let lines = wrap_paragraph(text, 10);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| l.chars().count() <= 10));
    }

    #[test]
    fn hard_breaks_overlong_words() {
        let lines = wrap_paragraph("supercalifragilistic", 5);
        assert!(lines.iter().all(|l| l.chars().count() <= 5));
        assert!(lines.join("").contains("supercalifragilistic"));
    }

    #[test]
    fn renders_pdf_bytes() {
        let data = sample_data();
        let (bytes, page_count) = export_meeting_to_pdf(&data).expect("PDF should be generated");
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[0..4], b"%PDF");
        assert_eq!(page_count, 1);
    }

    #[test]
    fn renders_markdown_table_in_pdf() {
        // Test that markdown tables in list sections are rendered
        let table_content = "| Owner | Task | Due |\n| --- | --- | --- |\n| Ana | Finish doc | 2026-07-25 |\n| Bruno | Review budget | 2026-07-26 |";
        let data = MeetingExportData {
            meeting_id: "test-123".into(),
            meeting_title: "Test Meeting".into(),
            created_at: "2026-06-29T14:00:00Z".into(),
            duration: Some("00:30:00".into()),
            attendees: Some("Ana, Bruno".into()),
            template_name: "Test Template".into(),
            sections: vec![
                SectionContent {
                    title: "Action Items".into(),
                    format: "list".into(),
                    content: table_content.into(),
                    item_format: None,
                },
            ],
        };
        let (bytes, page_count) = export_meeting_to_pdf(&data).expect("PDF should be generated");
        assert!(!bytes.is_empty());
        assert_eq!(page_count, 1);
    }

    #[test]
    fn renders_markdown_table_without_header_in_pdf() {
        // Test table with only data rows (no header/separator)
        let table_content = "| Ana | Finish doc | 2026-07-25 |\n| Bruno | Review budget | 2026-07-26 |";
        let data = MeetingExportData {
            meeting_id: "test-123".into(),
            meeting_title: "Test Meeting".into(),
            created_at: "2026-06-29T14:00:00Z".into(),
            duration: Some("00:30:00".into()),
            attendees: Some("Ana, Bruno".into()),
            template_name: "Test Template".into(),
            sections: vec![
                SectionContent {
                    title: "Action Items".into(),
                    format: "list".into(),
                    content: table_content.into(),
                    item_format: None,
                },
            ],
        };
        let (bytes, page_count) = export_meeting_to_pdf(&data).expect("PDF should be generated");
        assert!(!bytes.is_empty());
        assert_eq!(page_count, 1);
    }

    #[test]
    fn renders_markdown_table_header_only_in_pdf() {
        // Test table with header + data but no separator row (|---|---|)
        let table_content = "| Owner | Task | Due |\n| Ana | Finish doc | 2026-07-25 |\n| Bruno | Review budget | 2026-07-26 |";
        let data = MeetingExportData {
            meeting_id: "test-123".into(),
            meeting_title: "Test Meeting".into(),
            created_at: "2026-06-29T14:00:00Z".into(),
            duration: Some("00:30:00".into()),
            attendees: Some("Ana, Bruno".into()),
            template_name: "Test Template".into(),
            sections: vec![
                SectionContent {
                    title: "Action Items".into(),
                    format: "list".into(),
                    content: table_content.into(),
                    item_format: None,
                },
            ],
        };
        let (bytes, page_count) = export_meeting_to_pdf(&data).expect("PDF should be generated");
        assert!(!bytes.is_empty());
        assert_eq!(page_count, 1);
    }

    #[test]
    fn render_table_grid_renders_small_3col_table_as_grid() {
        // Sanity check for the grid renderer: a well-formed 3-col table
        // with a separator must return `true` (not fall back). Catches
        // the mm_per_char and measure_longest_word inertness bugs.
        let (doc, page, layer) = PdfDocument::new(
            "grid-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");
        let mut ctx = RenderContext::new(&doc, page, layer, fonts, "grid-test".into());

        let table = "| Owner | Task | Due |\n\
                     | --- | --- | --- |\n\
                     | Ana | Finish doc | 2026-07-25 |\n\
                     | Bruno | Review budget | 2026-07-26 |";
        let rendered = render_table_grid(&mut ctx, table, BODY_SIZE_PT, SECTION_HEADING_SIZE_PT, None);
        assert!(rendered, "render_table_grid should return true for a well-formed 3-col table");
    }

    #[test]
    fn checkbox_list_to_pipe_table_extracts_owner_and_due() {
        // ponytail: one small test that fails if the parser regresses on
        // `[[Owner]]` extraction, `Due:` extraction, missing-token handling,
        // or column count. Catches the three branches of the converter.
        let content = "- [ ] Finish quarterly report [[Ana Lopez]] Due: 2026-07-25\n\
                       - [x] Review budget - [[Bruno]] Due: 2026-07-30\n\
                       - [ ] No owner here Due: 2026-08-01";
        let table = checkbox_list_to_pipe_table(content);
        let lines: Vec<&str> = table.lines().collect();
        // header + separator + 3 data rows
        assert_eq!(lines.len(), 5, "expected 5 lines (header, sep, 3 rows), got: {:?}", lines);
        assert_eq!(lines[0], "| Task | Owner | Due |");
        assert_eq!(lines[1], "| --- | --- | --- |");
        // row 1: Ana + Due 2026-07-25, task is "Finish quarterly report"
        assert!(lines[2].contains("Ana Lopez"), "row1 missing owner: {}", lines[2]);
        assert!(lines[2].contains("2026-07-25"), "row1 missing due: {}", lines[2]);
        assert!(lines[2].contains("Finish quarterly report"), "row1 missing task: {}", lines[2]);
        // row 2: Bruno, checked item (`- [x]`) should still parse
        assert!(lines[3].contains("Bruno"), "row2 missing owner: {}", lines[3]);
        assert!(lines[3].contains("2026-07-30"), "row2 missing due: {}", lines[3]);
        // row 3: no `[[Owner]]`, owner cell should be empty (between `| |`)
        assert!(lines[4].contains("No owner here"), "row3 missing task: {}", lines[4]);
        assert!(lines[4].contains("2026-08-01"), "row3 missing due: {}", lines[4]);
        // owner cell empty: `| ... |  | 2026-08-01 |` (the middle `|  |` pattern)
        assert!(lines[4].matches('|').count() >= 4, "row3 should have empty owner cell: {}", lines[4]);
    }

    #[test]
    fn looks_like_checkbox_list_requires_majority() {
        assert!(looks_like_checkbox_list("- [ ] a [[X]] Due: 1\n- [ ] b [[Y]] Due: 2"));
        assert!(looks_like_checkbox_list("- [x] a\n- [ ] b\n- [ ] c"));
        // minority checkboxes should NOT trigger
        assert!(!looks_like_checkbox_list("- [ ] stray\n- a\n- b\n- c\n- d"));
        // single line shouldn't trigger
        assert!(!looks_like_checkbox_list("- [ ] only one line here"));
    }

    #[test]
    fn render_table_grid_falls_back_when_row_taller_than_page() {
        // A single cell wrapping to more lines than fit on one page must
        // fall back (return false) rather than paginate mid-row, which
        // would leave borders on the old page and borderless overflow
        // text on the new one.
        let (doc, page, layer) = PdfDocument::new(
            "grid-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");
        let mut ctx = RenderContext::new(&doc, page, layer, fonts, "grid-test".into());

        let long_cell = "word ".repeat(2000);
        let table = format!(
            "| A | B |\n| --- | --- |\n| short | {} |",
            long_cell.trim()
        );
        let rendered = render_table_grid(&mut ctx, &table, BODY_SIZE_PT, SECTION_HEADING_SIZE_PT, None);
        assert!(!rendered, "render_table_grid should fall back for a row taller than one page");
    }

    #[test]
    fn draw_table_row_does_not_paginate_mid_cell_when_row_fits() {
        // Regression lock for the "lone text without borders" bug:
        // `_write_wrapped_impl` used to paginate against `CONTENT_BOTTOM_MM`
        // whenever its preemptive check fired, even when called from inside
        // a table cell whose row borders were already drawn on the current
        // page. That split a row's text across pages: lines 1..n on the
        // old page (inside borders), line n+1 in a fresh page with NO
        // borders, followed by the header-repeat logic emitting another
        // header — visually a "doubled header". The fix is
        // `_write_wrapped_impl_opts(allow_paginate=false)` in `draw_table_row`.
        //
        // This test forces the trigger condition: a row whose multi-line
        // cell content is predicted (by `compute_row_height`) to fit just
        // barely above `CONTENT_BOTTOM_MM`, where the OLD preemptive check
        // `y - line_height < CONTENT_BOTTOM_MM` would have fired for the
        // last line. We verify that no new page is created.
        let (doc, page, layer) = PdfDocument::new(
            "row-pag-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");
        let mut ctx = RenderContext::new(&doc, page, layer, fonts, "row-pag-test".into());

        // We need a row whose bottom_border sits exactly at CONTENT_BOTTOM_MM
        // (worst-case fit), so the last cell line's baseline is just above
        // the floor. Construct an arbitrary multi-line cell, measure its
        // row_height, then position cursor_y so bottom_y == CONTENT_BOTTOM_MM.
        let col_widths = [40.0, 40.0];
        let table_right = CONTENT_LEFT_MM + 80.0 + 1.5; // matches col_left math below
        let col_left = [CONTENT_LEFT_MM + 0.5, CONTENT_LEFT_MM + 40.5 + 0.5];
        // Multi-line cell: long enough to wrap to ~4 lines at 40mm width.
        // 4 lines * LINE_HEIGHT_BODY (4.6) = 18.4mm span; row_height ≈
        // 2*1.5 + 2.97 + 18.4 + 0.74 ≈ 25.11mm.
        let cell_text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
        let row = vec![cell_text.to_string(), "short b".to_string()];
        let row_h = compute_row_height(&row, &col_widths, BODY_SIZE_PT, TABLE_CELL_PADDING_MM);
        assert!(row_h > 6.70 && row_h < 30.0, "expected multi-line row (~25mm), got {row_h}mm");

        // Position cursor so bottom_border == CONTENT_BOTTOM_MM exactly:
        // cursor_y - row_h = CONTENT_BOTTOM_MM  ->  cursor_y = CONTENT_BOTTOM_MM + row_h.
        ctx.cursor_y = CONTENT_BOTTOM_MM + row_h;

        // Sanity: caller's pre-check passes (row fits on this page).
        assert!(
            ctx.cursor_y - row_h >= CONTENT_BOTTOM_MM,
            "pre-check must pass: {} >= {}",
            ctx.cursor_y - row_h,
            CONTENT_BOTTOM_MM
        );

        let page_before = ctx.page_number;
        draw_table_row(
            &mut ctx,
            &row,
            &col_left,
            &col_widths,
            table_right,
            BODY_SIZE_PT,
            FontWeight::Regular,
            TABLE_CELL_PADDING_MM,
            row_h,
        );
        // The bug would have created a new page (page_number incremented)
        // and the last cell line would have been written on page 2 with
        // no borders. With the fix, we stay on page 1.
        assert_eq!(
            ctx.page_number, page_before,
            "draw_table_row must not paginate mid-row when the row fits the page"
        );
        // Cursor advanced by exactly row_h, leaving bottom_border at CONTENT_BOTTOM_MM.
        assert!(
            (ctx.cursor_y - (CONTENT_BOTTOM_MM)).abs() < 0.01,
            "cursor_y should be at CONTENT_BOTTOM_MM after the row; got {}",
            ctx.cursor_y
        );
    }

    #[test]
    fn render_table_grid_strips_bold_markers_from_item_format_header() {
        // Templates bold header cells (`| **Owner** | ...`). The grid
        // draws headers bold already; the parsed header must not keep
        // literal asterisks. Assert via the synthesized header path.
        let (doc, page, layer) = PdfDocument::new(
            "grid-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");
        let mut ctx = RenderContext::new(&doc, page, layer, fonts, "grid-test".into());

        // No separator line => header synthesized from item_format.
        let table = "| Ana | Finish doc | 2026-07-25 |\n| Bruno | Review budget | 2026-07-26 |";
        let item_format = "| **Owner** | **Task** | **Due** |\n| --- | --- | --- |";
        let rendered = render_table_grid(
            &mut ctx,
            table,
            BODY_SIZE_PT,
            SECTION_HEADING_SIZE_PT,
            Some(item_format),
        );
        assert!(rendered, "render_table_grid should render with a bold-marked item_format header");
    }

    #[test]
    fn render_table_grid_draws_grid_rows_with_padding() {
        // Renders a 3-col explicit-header table of short cells and asserts
        // the grid path ran by checking cursor advance: only `draw_table_row`
        // (the border+padding path) advances with per-row padding; the plain
        // fallback advances by bare LINE_HEIGHT_BODY per wrapped line.
        //
        // Math (single-line cells, columns wide enough — col widths follow
        // from the available_mm / col_min_chars algorithm):
        //   row_h = 2*v_padding + font_size*0.3528 (ascent+descent, span=0)
        //   header_h (font 13pt)  = 3.0 + 4.586 = 7.586
        //   body_h   (font 10.5pt) = 3.0 + 3.704 = 6.704
        //   total = 7.586 + 2*6.704 = 20.994
        // Pixel-level border verification is item 38 (visual export).
        let (doc, page, layer) = PdfDocument::new(
            "grid-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");
        let mut ctx = RenderContext::new(&doc, page, layer, fonts, "grid-test".into());
        let start_y = ctx.cursor_y;

        let table = "| Owner | Task | Due |\n\
                     | --- | --- | --- |\n\
                     | Ana | Finish doc | 2026-07-25 |\n\
                     | Bruno | Review budget | 2026-07-26 |";
        let rendered = render_table_grid(&mut ctx, table, BODY_SIZE_PT, SECTION_HEADING_SIZE_PT, None);
        assert!(rendered, "well-formed 3-col table should render as a grid");
        let advance = start_y - ctx.cursor_y;
        assert!(
            (advance - 20.99).abs() < 0.05,
            "grid advance should equal 7.586 + 2*6.704 = ~20.99mm (got {advance}"
        );
    }

    #[test]
    fn compute_row_height_grows_when_cell_wraps() {
        // A long 50-word cell in a narrow 40mm column must wrap to many
        // lines, so row height must exceed a single line. A short cell in
        // a wide 120mm column fits one line, proving height is
        // column-width-driven (not a constant).
        let long_cell = "word ".repeat(50);
        let long_row = vec!["x".to_string(), long_cell.trim().to_string()];

        // Single-line row height under the new formula:
        // 2*v_pad + ascent + descent (span=0 for single line).
        let single_line = 2.0 * TABLE_CELL_PADDING_MM
            + BODY_SIZE_PT * 0.3528 * (0.8 + 0.2);

        let narrow = compute_row_height(&long_row, &[30.0, 40.0], BODY_SIZE_PT, TABLE_CELL_PADDING_MM);
        assert!(
            narrow > single_line + 0.001,
            "narrow column should wrap -> height > single-line ({narrow} vs {single_line})"
        );

        let short_row = vec!["x".to_string(), "hi".to_string()];
        let wide = compute_row_height(&short_row, &[40.0, 120.0], BODY_SIZE_PT, TABLE_CELL_PADDING_MM);
        assert!(
            (wide - single_line).abs() < 0.001,
            "short cell in wide column should fit one line ({wide} vs {single_line})"
        );
    }

    #[test]
    fn render_table_grid_paginates_and_repeats_header() {
        // 40 short rows force a page break. Math (revised row-height formula):
        //   page_content = CONTENT_TOP (277) - CONTENT_BOTTOM (30) = 247mm
        //   body row_h = 2*1.5 + 10.5*0.3528 = 6.704mm (single line, span=0)
        //   header_h   = 2*1.5 + 13*0.3528   = 7.586mm
        //   page 1: header (7.586) + 35 rows (234.65) -> cursor = 277 - 242.24 = 34.76
        //           row 36 check: 34.76 - 6.704 = 28.05 < 30 -> break before row 36.
        //   page 2: header repeat (7.586) + 5 rows (33.52) -> cursor = 277 - 41.11 = ~235.89
        //   WITHOUT header repeat, page-2 cursor would be 277 - 5*6.704 = ~243.5.
        //   Asserting cursor ≈ 235.89 pins BOTH pagination AND header repetition.
        let (doc, page, layer) = PdfDocument::new(
            "grid-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");
        let mut ctx = RenderContext::new(&doc, page, layer, fonts, "grid-test".into());

        let mut table = String::from("| A | B | C |\n| --- | --- | --- |\n");
        for i in 0..40 {
            table.push_str(&format!("| r{i}a | r{i}b | r{i}c |\n"));
        }
        let rendered = render_table_grid(&mut ctx, table.trim(), BODY_SIZE_PT, SECTION_HEADING_SIZE_PT, None);
        assert!(rendered, "table should render as grid");
        assert_eq!(ctx.page_number, 2, "should have created exactly one continuation page");
        let final_y = ctx.cursor_y;
        assert!(
            (final_y - 235.89).abs() < 0.05,
            "page-2 cursor should be ~235.89 (header repeated + 5 rows); got {final_y}"
        );
    }

    #[test]
    fn render_table_grid_starts_on_new_page_when_only_header_plus_one_row_fits() {
        // Regression lock for the "doubled header on overflow" bug:
        // starting a table near the bottom of a page such that only the
        // header + exactly one data row would fit → pre-row check
        // would paginate before row 2 and re-emit the header, visually a
        // "doubled header". The fix paginates BEFORE the table's header
        // is drawn on the cramped page, so the table starts on a fresh
        // page with header + multiple rows together.
        //
        // Setup: put cursor near page bottom so only header + 1 row fits.
        //   header_h ≈ 7.586mm, row_h ≈ 6.704mm → header + 1 row ≈ 14.29mm.
        //   Set cursor_y = CONTENT_BOTTOM_MM + 14.29 + 1mm slack = 30 + 15.29 = 45.29mm.
        //   Then header_h + first_row_h + second_row_h ≈ 20.99mm > 15.29mm slack
        //   → guard fires → new_page() before drawing anything.
        //   On the fresh page (cursor = 277), header + 3 rows ≈ 27.7mm fits
        //   easily, so no further pagination: page_number==2, cursor ≈
        //   277 - 27.7 = ~249.3mm.
        let (doc, page, layer) = PdfDocument::new(
            "grid-orphan-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");
        let mut ctx = RenderContext::new(&doc, page, layer, fonts, "grid-orphan-test".into());

        // 3 short rows; on a fresh page header + 3 rows ≈ 27.7mm fits well.
        let table = "| A | B | C |\n\
                     | --- | --- | --- |\n\
                     | r0a | r0b | r0c |\n\
                     | r1a | r1b | r1c |\n\
                     | r2a | r2b | r2c |";
        // Position cursor so only header + 1 row fits on the page.
        let header_h = 2.0 * TABLE_HEADER_PADDING_MM + SECTION_HEADING_SIZE_PT * 0.3528;
        let row_h = 2.0 * TABLE_CELL_PADDING_MM + BODY_SIZE_PT * 0.3528;
        ctx.cursor_y = CONTENT_BOTTOM_MM + header_h + row_h + 1.0; // ~45.29mm

        let rendered = render_table_grid(&mut ctx, table, BODY_SIZE_PT, SECTION_HEADING_SIZE_PT, None);
        assert!(rendered, "table should render as grid");

        // The guard must have fired: nothing should be on page 1, the
        // whole table (header + 3 rows) on page 2.
        assert_eq!(ctx.page_number, 2, "guard should have paginated before drawing the table");

        // Page 2: header (~7.586) + 3 rows (3 * ~6.704 = 20.11mm) →
        // cursor = 277 - 27.70 = ~249.30mm.
        let expected = CONTENT_TOP_MM - header_h - 3.0 * row_h;
        assert!(
            (ctx.cursor_y - expected).abs() < 0.1,
            "page-2 cursor should be ~{expected:.3} (header + 3 rows); got {}",
            ctx.cursor_y
        );
    }

    #[test]
    fn render_table_grid_header_synthesis_fallbacks() {
        // Single ctx, three independent calls covering the three sub-paths
        // of item_format header synthesis (happy path covered by the
        // bold-markers test above):
        //   (a) col-count mismatch      -> false (fallback + warn)
        //   (b) item_format not pipe     -> false
        //   (c) no item_format, no sep   -> true (generic "Col N" header)
        let (doc, page, layer) = PdfDocument::new(
            "grid-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");
        let mut ctx = RenderContext::new(&doc, page, layer, fonts, "grid-test".into());

        // (a) 2-col item_format, 3-col data, no separator -> mismatch
        let table_3col = "| a | b | c |\n| d | e | f |";
        let rendered = render_table_grid(
            &mut ctx,
            table_3col,
            BODY_SIZE_PT,
            SECTION_HEADING_SIZE_PT,
            Some("| **X** | **Y** |\n| --- | --- |"),
        );
        assert!(!rendered, "(a) col-count mismatch should fall back");

        // (b) item_format does not start with '|'
        let rendered = render_table_grid(
            &mut ctx,
            table_3col,
            BODY_SIZE_PT,
            SECTION_HEADING_SIZE_PT,
            Some("Owner, Task, Due"),
        );
        assert!(!rendered, "(b) non-pipe item_format should fall back");

        // (c) no separator, no item_format -> generic "Col N" header, grid renders
        let rendered = render_table_grid(
            &mut ctx,
            table_3col,
            BODY_SIZE_PT,
            SECTION_HEADING_SIZE_PT,
            None,
        );
        assert!(rendered, "(c) generic synthesized header should render as grid");
    }

    #[test]
    fn render_table_grid_explicit_header_ignores_item_format() {
        // Regression: an explicit header separator MUST win; item_format
        // is never consulted in that branch. Proven by passing a 2-col
        // item_format to a 3-col table WITH a separator: with a separator
        // the grid renders (true); without one (test 4a) it falls back
        // (false) on the same data. This true result proves precedence.
        let (doc, page, layer) = PdfDocument::new(
            "grid-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");
        let mut ctx = RenderContext::new(&doc, page, layer, fonts, "grid-test".into());

        let table = "| H1 | H2 | H3 |\n\
                     | --- | --- | --- |\n\
                     | a | b | c |";
        let rendered = render_table_grid(
            &mut ctx,
            table,
            BODY_SIZE_PT,
            SECTION_HEADING_SIZE_PT,
            Some("| **X** | **Y** |\n| --- | --- |"),  // 2-col, mismatches data
        );
        assert!(rendered, "explicit header must win; matched-col-count item_format should be ignored");
    }

    #[test]
    fn render_table_grid_fallback_paths_still_render_content() {
        // Locks review fix #1: every `return false` path in render_table_grid
        // has already called render_markdown_table, so the content is
        // never silently dropped. Each case uses a fresh ctx so the cursor
        // delta is unambiguous (cursor starts at CONTENT_TOP_MM each time).
        let new_ctx = || -> RenderContext {
            let (doc, page, layer) = PdfDocument::new(
                "grid-test",
                Mm(PAGE_WIDTH_MM),
                Mm(PAGE_HEIGHT_MM),
                "page-1",
            );
            // ponytail: keep the PdfDoc alive for the ctx's lifetime.
            // Constructed leak is fine for a test; the doc is small.
            let doc_box: &'static PdfDocumentReference = Box::leak(Box::new(doc));
            let fonts = FontSet::load(doc_box).expect("font load");
            RenderContext::new(doc_box, page, layer, fonts, "grid-test".into())
        };

        // Case A: 340 columns -> border_total = 341*0.5 = 170.5 > 170 = CONTENT_WIDTH,
        // so available_mm <= 0 path (the one fix #1 patched).
        let mut ctx = new_ctx();
        let mut header_row = String::from("|");
        let mut data_row = String::from("|");
        for i in 0..340 {
            header_row.push_str(&format!(" h{i} |"));
            data_row.push_str(&format!(" d{i} |"));
        }
        let table = format!("{header_row}\n|{sep}|\n{data_row}", sep = "---|".repeat(339) + "---");
        let before_a = ctx.cursor_y;
        let rendered = render_table_grid(&mut ctx, &table, BODY_SIZE_PT, SECTION_HEADING_SIZE_PT, None);
        assert!(!rendered, "340-col table should fall back via available_mm<=0 path");
        assert!(
            ctx.cursor_y < before_a,
            "fallback case A should have rendered content (cursor {before_a} -> {})",
            ctx.cursor_y
        );

        // Case B: col-count mismatch between item_format and data.
        let mut ctx = new_ctx();
        let before_b = ctx.cursor_y;
        let rendered = render_table_grid(
            &mut ctx,
            "| a | b | c |\n| d | e | f |",
            BODY_SIZE_PT,
            SECTION_HEADING_SIZE_PT,
            Some("| **X** | **Y** |\n| --- | --- |"),
        );
        assert!(!rendered, "mismatch case should fall back");
        assert!(
            ctx.cursor_y < before_b,
            "fallback case B should have rendered content (cursor {before_b} -> {})",
            ctx.cursor_y
        );
    }

    #[test]
    fn render_list_renders_prose_around_table() {
        // Finding-5 fix: a section that mixes prose with a pipe table must
        // render BOTH. Measured by cursor advance: mixed content advance
        // minus table-only advance should account for ~2 prose lines
        // (lead-in + trailing), i.e. >= 2 * LINE_HEIGHT_BODY.
        let (doc, page, layer) = PdfDocument::new(
            "grid-test",
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            "page-1",
        );
        let fonts = FontSet::load(&doc).expect("font load");

        // Table-only baseline.
        let mut ctx_table_only = RenderContext::new(&doc, page, layer, fonts.clone(), "t".into());
        let table_only = "| A | B |\n| --- | --- |\n| 1 | 2 |";
        let start_table = ctx_table_only.cursor_y;
        render_list(&mut ctx_table_only, table_only, None);
        let table_advance = start_table - ctx_table_only.cursor_y;

        // Mixed: prose + table + prose.
        let mut ctx_mixed = RenderContext::new(&doc, page, layer, fonts, "t".into());
        let mixed = "See below:\n| A | B |\n| --- | --- |\n| 1 | 2 |\nThat is all.";
        let start_mixed = ctx_mixed.cursor_y;
        render_list(&mut ctx_mixed, mixed, None);
        let mixed_advance = start_mixed - ctx_mixed.cursor_y;

        let prose_advance = mixed_advance - table_advance;
        assert!(
            prose_advance >= 2.0 * LINE_HEIGHT_BODY - 0.05,
            "mixed content should render ~2 prose lines beyond the table; got {prose_advance} extra mm"
        );
    }

    #[test]
    fn split_paragraphs_strips_empty_lines() {
        let p = split_paragraphs("a\n\nb\n");
        assert_eq!(p, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_bold_segments_no_markers_returns_single_segment() {
        let segs = split_bold_segments("no bold here", FontWeight::Regular);
        assert_eq!(segs, vec![(FontWeight::Regular, "no bold here")]);
    }

    #[test]
    fn split_bold_segments_simple_pair() {
        let segs = split_bold_segments("hello **world** end", FontWeight::Regular);
        assert_eq!(
            segs,
            vec![
                (FontWeight::Regular, "hello "),
                (FontWeight::Bold, "world"),
                (FontWeight::Regular, " end"),
            ]
        );
    }

    #[test]
    fn split_bold_segments_multiple_pairs() {
        let segs = split_bold_segments("**a** and **b**", FontWeight::Regular);
        assert_eq!(
            segs,
            vec![
                (FontWeight::Bold, "a"),
                (FontWeight::Regular, " and "),
                (FontWeight::Bold, "b"),
            ]
        );
    }

    #[test]
    fn split_bold_segments_unbalanced_asterisks_fall_back_to_base() {
        // One `**` only -> no pair, dangling mark. The "bold" segment
        // (everything after the lone `**`) gets demoted to base weight
        // so the user sees literal text rather than a sliced half-bold.
        let segs = split_bold_segments("foo **bar baz", FontWeight::Regular);
        assert_eq!(
            segs,
            vec![(FontWeight::Regular, "foo "), (FontWeight::Regular, "bar baz")]
        );
    }

    #[test]
    fn split_bold_segments_preserves_base_weight_for_bold_context() {
        // Section-title / heading path passes base=Bold; segments still
        // get Bold weight even outside the `**...**` markers.
        let segs = split_bold_segments("Section Title", FontWeight::Bold);
        assert_eq!(segs, vec![(FontWeight::Bold, "Section Title")]);
    }

    #[test]
    fn split_bold_segments_adjacent_pairs_alternate() {
        // `**a**b**c**` => Bold a / Regular b / Bold c, alternating toggling.
        let segs = split_bold_segments("**a**b**c**", FontWeight::Regular);
        assert_eq!(
            segs,
            vec![
                (FontWeight::Bold, "a"),
                (FontWeight::Regular, "b"),
                (FontWeight::Bold, "c"),
            ]
        );
    }

    #[test]
    fn split_bold_segments_triple_asterisk_demotes_dangling() {
        // `***` (three asterisks): first `**` toggles on (consumes 2 bytes),
        // the trailing `*` is left unpaired. Dangling text demotes to
        // base weight (Regular) per the parser's contract. The lone `*`
        // is rendered as literal text — somewhat surprising for a
        // stray `***` separator but consistent with the unbalanced case.
        // ponytail: ceiling — adjacent-`*` sequences ≥ 3 are rare in
        // meeting-summary output (LLMs use `**pair**` markers); upgrade
        // path is to scan for a maximal even prefix of `*` if `***`
        // becomes a real source of confusion.
        let segs = split_bold_segments("***", FontWeight::Regular);
        assert_eq!(segs, vec![(FontWeight::Regular, "*")]);
    }

    #[test]
    fn split_bold_segments_preserves_trailing_text_after_last_pair() {
        let segs = split_bold_segments("text **bold** trailing", FontWeight::Regular);
        assert_eq!(
            segs,
            vec![
                (FontWeight::Regular, "text "),
                (FontWeight::Bold, "bold"),
                (FontWeight::Regular, " trailing"),
            ]
        );
    }

    #[test]
    fn chars_per_line_is_positive() {
        assert!(approx_chars_per_line(BODY_SIZE_PT, CONTENT_WIDTH_MM) > 10);
    }

    #[test]
    fn font_metrics_loaded_and_glyph_widths_sane() {
        // ponytail: one runnable check for the hand-rolled TTF/cmap/hmtx
        // parser. Fails loud if the metrics table fails to parse (cmap
        // malformed, UPM missing, etc.) or if glyph-width relationships
        // are inverted (W narrower than i — sign of a wrong-table index).
        let m = font_metrics().expect("DejaVuSans metrics should load from templates/fonts/");
        assert!(m.units_per_em > 0, "unitsPerEm must be positive (got {})", m.units_per_em);
        assert_eq!(m.units_per_em, 2048, "DejaVuSans UPM is 2048");
        // Cmap must map at least the basic ASCII set and the Portuguese
        // accented vowels used in real meetings.
        assert!(m.glyph_index('A' as u32).is_some(), "ASCII A must map to a glyph");
        assert!(m.glyph_index('z' as u32).is_some(), "ASCII z must map to a glyph");
        assert!(m.glyph_index('á' as u32).is_some(), "á must map to a glyph (PT-BR/ES content)");
        assert!(m.glyph_index('ç' as u32).is_some(), "ç must map to a glyph");
        // Real-width wrap sanity: W is wider than i.
        let w_w = m.text_width_mm("W", BODY_SIZE_PT);
        let w_i = m.text_width_mm("i", BODY_SIZE_PT);
        assert!(w_w > w_i, "W ({w_w}mm) should be wider than i ({w_i}mm) at {BODY_SIZE_PT}pt");
        // text_width_mm is monotonic in length: "aaaa" wider than "aa".
        assert!(m.text_width_mm("aaaa", BODY_SIZE_PT) > m.text_width_mm("aa", BODY_SIZE_PT));
        // A reasonable sentence must be measurable: width of "hello world"
        // at body size should be a few mm and longer than "hello".
        let s1 = m.text_width_mm("hello", BODY_SIZE_PT);
        let s2 = m.text_width_mm("hello world", BODY_SIZE_PT);
        assert!(s1 > 0.0 && s2 > s1, "longer text must be wider ({s1} vs {s2})");
        // Bullet glyph (U+2022) is what `render_list`'s fallback path
        // emits on every line ("• {bullet_text}"); if it's missing from
        // the format-4 cmap, every bullet wraps with the fallback
        // advance and bullet lists mis-measure. ponytail: ceiling —
        // format-4 only covers BMP; if meeting notes ever contain
        // supplementary-plane chars (emoji), upgrade to format-12.
        assert!(m.glyph_index(0x2022).is_some(), "U+2022 bullet must map to a glyph");
        // Bold metrics must load (we ship DejaVuSans-Bold.ttf alongside
        // DejaVuSans.ttf) and a Bold glyph must measure at least as wide
        // as the Regular glyph of the same char. `write_line_at_segments`
        // picks bold metrics for bold segments; if bold metrics silently
        // fall back to None we'd revert to the horizontal-overlap bug
        // (Regular advance undercounts rendered Bold width → next segment
        // starts too far left → overlaps the Bold tail).
        let mb = bold_font_metrics().expect(
            "DejaVuSans-Bold metrics should load from templates/fonts/ \
             (write_line_at_segments needs them to stop Bold tails overlapping Regular text)",
        );
        let reg_a = m.text_width_mm("a", BODY_SIZE_PT);
        let bold_a = mb.text_width_mm("a", BODY_SIZE_PT);
        assert!(
            bold_a >= reg_a,
            "Bold 'a' ({bold_a}mm) must be >= Regular 'a' ({reg_a}mm); \
             if equal the bold TTF probably failed to parse and fell back"
        );
    }

    #[test]
    fn formats_iso_date_human_readable() {
        let formatted = format_date_human("2026-06-29T14:00:00Z");
        assert!(formatted.contains("2026-06-29"));
    }

    #[test]
    fn returns_iso_unchanged_when_unparseable() {
        let formatted = format_date_human("not-a-date");
        assert_eq!(formatted, "not-a-date");
    }
}
