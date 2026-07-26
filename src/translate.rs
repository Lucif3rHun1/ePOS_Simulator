//! Translate ePOS-Print XML into an ESC/POS byte stream.
//!
//! Single-pass state machine. Walks the XML with `quick_xml::Reader` and emits
//! ESC/POS bytes as elements are encountered. Text content is captured
//! directly from `Event::Text` events in the main loop, so nested patterns
//! like `<text><line><content>x</content></line></text>` and flat chardata
//! `<text>x</text>` are both handled without recursion.

use base64::Engine;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::escpos;
use crate::soap;
use tracing::warn;

/// Default printer paper width in dots (576 = 80mm at 203dpi).
pub const DEFAULT_PAPER_WIDTH: usize = 576;

/// Default character codepage — CP437 is the power-on default table on
/// virtually all ESC/POS thermal printers.
pub const DEFAULT_CODEPAGE: &str = "CP437";

#[derive(Debug, Clone)]
pub struct Options {
    pub verbose: bool,
    pub allow_drawer: bool,
    pub strict_xml: bool,
    /// Printer's physical paper width in dots, used to size raster image
    /// bands (see `<image>` handling). Not related to any XML attribute —
    /// the ePOS-Print `<image>` element's own `width`/`height` attributes
    /// describe the image itself, in dots.
    pub paper_width: usize,
    /// Character codepage `<text>` content is transcoded to before being
    /// sent (see [`crate::codepage`]) — thermal printers interpret bytes
    /// >= 0x80 per their active character table, not UTF-8.
    pub codepage: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            verbose: false,
            allow_drawer: false,
            strict_xml: false,
            paper_width: DEFAULT_PAPER_WIDTH,
            codepage: DEFAULT_CODEPAGE.to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TranslateError {
    #[error("no epos-print content found")]
    NotFound,
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("base64 decode failed: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error(transparent)]
    Soap(#[from] soap::ParseError),
    #[error("unknown epos-print element: <{0}>")]
    UnknownElement(String),
    #[error("missing required attribute: {0} on <{1}>")]
    MissingAttr(&'static str, String),
}

/// Translate a request body (raw or SOAP-wrapped) into ESC/POS bytes.
pub fn translate(data: &[u8], opts: Options) -> Result<Vec<u8>, TranslateError> {
    let body = soap::parse(data)?;
    translate_inner(body.as_bytes(), opts)
}

fn translate_inner(body: &[u8], opts: Options) -> Result<Vec<u8>, TranslateError> {
    // Wrap so XML is well-formed even for a single self-closing element.
    let mut wrapped = Vec::with_capacity(body.len() + 13);
    wrapped.extend_from_slice(b"<root>");
    wrapped.extend_from_slice(body);
    wrapped.extend_from_slice(b"</root>");

    let mut reader = Reader::from_reader(&wrapped[..]);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut out = Vec::new();
    out.extend_from_slice(&escpos::init());
    out.extend_from_slice(&escpos::select_codepage(crate::codepage::esc_t_table_number(&opts.codepage)));

    // State: which element context we are in. None means "at top level".
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Ctx {
        None,
        Text { align: Option<u8> },
        Line,    // inside <text> -> collecting <content> for the next line
        Content, // inside <line> -> collecting chardata for the current line
        Barcode { btype: u8, width: u8, height: u8, hri: u8, align: Option<u8>, buf: Vec<u8> },
        Symbol { ecc: u8, is_qr: bool, align: Option<u8>, buf: Vec<u8> },
        // <image width=".." height="..">base64-raster-data</image> — the
        // Epson spec (ePOS-Print XML User's Manual, "<image>") puts the
        // base64Binary raster data as element text content, not an
        // attribute. width/height here are the image's own dots.
        Image { width: usize, height: usize, align: Option<u8>, buf: Vec<u8> },
    }
    let mut ctx = Ctx::None;
    let mut pending_text = String::new();
    let mut pending_align_reset = false;
    let mut pending_style_reset = false;

    loop {
        buf.clear();
        let event = reader.read_event_into(&mut buf);
        match event {
            Err(e) => return Err(TranslateError::Xml(format!("{e} at pos {}", reader.buffer_position()))),
            Ok(Event::Eof) => break,
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                match name.as_str() {
                    "feedline" => out.extend_from_slice(&escpos::feed(1)),
                    "feed" => {
                        let n = attr_u8(&e, "line").unwrap_or(1);
                        out.extend_from_slice(&escpos::feed(n));
                    }
                    "cut" => {
                        // Epson ePOS-Print XML cut semantics:
                        //   <cut/>                -> partial cut (GS V m=1)
                        //   <cut type="feed"/>    -> feed N lines + partial cut
                        //   <cut type="no-feed"/> -> partial cut, no feed
                        //   <cut type="full"/>    -> full cut (GS V m=0)
                        let kind = attr_str(&e, "type").unwrap_or_default();
                        let feed_lines: u8 = if kind == "feed" { 3 } else { 0 };
                        let m: u8 = if kind == "full" { 0 } else { 1 };
                        if feed_lines > 0 {
                            out.extend_from_slice(&escpos::feed(feed_lines));
                        }
                        out.extend_from_slice(&escpos::cut(m));
                    }
                    "drawer" if opts.allow_drawer => {
                        out.extend_from_slice(&escpos::drawer(100, 250));
                    }
                    "pulse" if opts.allow_drawer => {
                        out.extend_from_slice(&escpos::pulse());
                    }
                    "image" => {
                        // Self-closing fallback for a `data` attribute some
                        // emitters may use; the real per-spec path (base64
                        // as element text content) goes through Ctx::Image
                        // in the Start/Text/End handling below.
                        let data = attr_str(&e, "data").unwrap_or_default();
                        if !data.is_empty() {
                            let width = attr_usize(&e, "width").unwrap_or(1).max(1);
                            let height = attr_usize(&e, "height").unwrap_or(1).max(1);
                            if let Ok(img) = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()) {
                                emit_aligned(&mut out, parse_align(&e), |out| {
                                    out.extend_from_slice(&escpos::raster_print(&img, width, height, opts.paper_width));
                                });
                            }
                        }
                    }
                    "barcode" => {
                        let btype = attr_str(&e, "type").as_deref().map(parse_barcode_type).unwrap_or(8);
                        let width = attr_u8(&e, "width").unwrap_or(3).clamp(2, 6);
                        let height = attr_u8(&e, "height").unwrap_or(100);
                        let hri = parse_hri(&e);
                        // For Empty, no inner chardata — use attribute only.
                        if let Some(data) = attr_str(&e, "data") {
                            let data = data.trim().to_string();
                            if !data.is_empty() && warn_if_invalid_barcode_data(btype, &data) {
                                emit_aligned(&mut out, parse_align(&e), |out| {
                                    out.extend_from_slice(&escpos::barcode(data.as_bytes(), btype, width, height, hri));
                                });
                            }
                        }
                    }
                    "symbol" => {
                        let ecc = attr_str(&e, "eccLevel").as_deref().map(parse_ecc_level).unwrap_or(3);
                        let is_qr = attr_str(&e, "type").as_deref().map(is_qr_symbol_type).unwrap_or(true);
                        if let Some(data) = attr_str(&e, "data") {
                            let data = data.trim().to_string();
                            if !data.is_empty() && is_qr {
                                emit_aligned(&mut out, parse_align(&e), |out| {
                                    out.extend_from_slice(&escpos::qr_code(data.as_bytes(), ecc, 4));
                                });
                            }
                        }
                    }
                    "text" => {
                        let align = match attr_str(&e, "align").as_deref() {
                            Some("center") => Some(1u8),
                            Some("right") => Some(2u8),
                            _ => None,
                        };
                        if let Some(a) = align {
                            out.extend_from_slice(&escpos::align(a));
                        }
                        if apply_text_style(&e, &mut out) {
                            reset_text_style(&mut out);
                        }
                        if align.is_some() {
                            out.extend_from_slice(&escpos::align(0));
                        }
                    }
                    other if opts.strict_xml => {
                        return Err(TranslateError::UnknownElement(other.to_string()));
                    }
                    _ => {}
                }
            }
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                match (&ctx, name.as_str()) {
                    (Ctx::None, "text") => {
                        let align = match attr_str(&e, "align").as_deref() {
                            Some("center") => Some(1u8),
                            Some("right") => Some(2u8),
                            _ => None,
                        };
                        if let Some(a) = align {
                            out.extend_from_slice(&escpos::align(a));
                            pending_align_reset = true;
                        }
                        if apply_text_style(&e, &mut out) {
                            pending_style_reset = true;
                        }
                        ctx = Ctx::Text { align };
                        pending_text.clear();
                    }
                    (Ctx::Text { .. }, "line") => {
                        ctx = Ctx::Line;
                    }
                    (Ctx::Line, "content") => {
                        ctx = Ctx::Content;
                    }
                    (Ctx::Text { .. }, "content") => {
                        // Some emitters put <content> directly under <text>.
                        ctx = Ctx::Content;
                    }
                    (Ctx::None, "barcode") => {
                        ctx = Ctx::Barcode {
                            btype: attr_str(&e, "type").as_deref().map(parse_barcode_type).unwrap_or(8),
                            width: attr_u8(&e, "width").unwrap_or(3).clamp(2, 6),
                            height: attr_u8(&e, "height").unwrap_or(100),
                            hri: parse_hri(&e),
                            align: parse_align(&e),
                            buf: Vec::new(),
                        };
                    }
                    (Ctx::None, "symbol") => {
                        ctx = Ctx::Symbol {
                            ecc: attr_str(&e, "eccLevel").as_deref().map(parse_ecc_level).unwrap_or(3),
                            is_qr: attr_str(&e, "type").as_deref().map(is_qr_symbol_type).unwrap_or(true),
                            align: parse_align(&e),
                            buf: Vec::new(),
                        };
                    }
                    (Ctx::None, "feedline") => out.extend_from_slice(&escpos::feed(1)),
                    (Ctx::None, "feed") => {
                        let n = attr_u8(&e, "line").unwrap_or(1);
                        out.extend_from_slice(&escpos::feed(n));
                    }
                    (Ctx::None, "cut") => out.extend_from_slice(&escpos::cut(0)),
                    (Ctx::None, "image") => {
                        ctx = Ctx::Image {
                            width: attr_usize(&e, "width").unwrap_or(1).max(1),
                            height: attr_usize(&e, "height").unwrap_or(1).max(1),
                            align: parse_align(&e),
                            buf: Vec::new(),
                        };
                    }
                    (Ctx::None, "drawer") => {
                        if opts.allow_drawer {
                            out.extend_from_slice(&escpos::drawer(100, 250));
                        }
                    }
                    (Ctx::None, "pulse") => {
                        if opts.allow_drawer {
                            out.extend_from_slice(&escpos::pulse());
                        }
                    }
                    (Ctx::None, "epos-print") | (Ctx::None, "root") | (Ctx::None, "parameter") => {
                        // wrapper — recurse by staying in Ctx::None
                    }
                    (Ctx::None, other) => {
                        if opts.strict_xml {
                            return Err(TranslateError::UnknownElement(other.to_string()));
                        }
                        // unknown element at top level — quick-xml advances
                        // naturally over the subtree via Start/End events.
                    }
                    (_ctx, _other) => {
                        // nested under another element — ignored
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                match (&ctx, name.as_str()) {
                    (Ctx::Content, "content") => {
                        // line content closed — back to text
                        ctx = Ctx::Text { align: None };
                    }
                    (Ctx::Text { .. }, "line") | (Ctx::Line, "line") => {
                        // close line
                        ctx = Ctx::Text { align: None };
                    }
                    (Ctx::Text { .. }, "text") => {
                        // flush text
                        let trimmed = pending_text.trim_start();
                        if !trimmed.is_empty() {
                            out.extend_from_slice(&crate::codepage::encode(trimmed, &opts.codepage));
                        }
                        pending_text.clear();
                        if pending_style_reset {
                            reset_text_style(&mut out);
                            pending_style_reset = false;
                        }
                        if pending_align_reset {
                            out.extend_from_slice(&escpos::align(0));
                            pending_align_reset = false;
                        }
                        ctx = Ctx::None;
                    }
                    (Ctx::Barcode { .. }, "barcode") => {
                        // ctx already destructured in match — re-read
                    }
                    _ => {}
                }
                // Handle barcode/symbol close + nested fallback
                if name == "barcode" {
                    if let Ctx::Barcode { btype, width, height, hri, align, buf } = std::mem::replace(&mut ctx, Ctx::None) {
                        let data = String::from_utf8_lossy(&buf).trim().to_string();
                        if !data.is_empty() && warn_if_invalid_barcode_data(btype, &data) {
                            emit_aligned(&mut out, align, |out| {
                                out.extend_from_slice(&escpos::barcode(data.as_bytes(), btype, width, height, hri));
                            });
                        }
                    }
                } else if name == "symbol" {
                    if let Ctx::Symbol { ecc, is_qr, align, buf } = std::mem::replace(&mut ctx, Ctx::None) {
                        let data = String::from_utf8_lossy(&buf).trim().to_string();
                        if !data.is_empty() && is_qr {
                            emit_aligned(&mut out, align, |out| {
                                out.extend_from_slice(&escpos::qr_code(data.as_bytes(), ecc, 4));
                            });
                        }
                    }
                } else if name == "image" {
                    if let Ctx::Image { width, height, align, buf } = std::mem::replace(&mut ctx, Ctx::None) {
                        let data = String::from_utf8_lossy(&buf);
                        let data = data.trim();
                        if !data.is_empty() {
                            let img = base64::engine::general_purpose::STANDARD.decode(data.as_bytes())?;
                            emit_aligned(&mut out, align, |out| {
                                out.extend_from_slice(&escpos::raster_print(&img, width, height, opts.paper_width));
                            });
                        }
                    }
                }
            }
            Ok(Event::Text(t)) => {
                let s = t.unescape().unwrap_or_default().into_owned();
                match ctx {
                    Ctx::Text { .. } | Ctx::Line => {
                        pending_text.push_str(&s);
                    }
                    Ctx::Content => {
                        pending_text.push_str(&s);
                    }
                    Ctx::Barcode { ref mut buf, .. } => buf.extend_from_slice(s.as_bytes()),
                    Ctx::Symbol { ref mut buf, .. } => buf.extend_from_slice(s.as_bytes()),
                    Ctx::Image { ref mut buf, .. } => buf.extend_from_slice(s.as_bytes()),
                    Ctx::None => {
                        // Top-level text outside any element — Odoo POS sometimes
                        // emits whitespace, just ignore.
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Reads `<text>`'s `em` (bold), `ul` (underline), `width`/`height`
/// (1-8x character scaling) attributes and emits the corresponding
/// ESC/POS bytes into `out`. Returns whether anything was applied, so
/// the caller knows whether a reset is needed when the element closes.
/// `align` attribute shared by `<image>`, `<barcode>`, and `<symbol>` (per
/// spec, setting it on any of these also affects the others — we just
/// read it per-element, which gives the same observable result).
fn parse_align(e: &quick_xml::events::BytesStart<'_>) -> Option<u8> {
    match attr_str(e, "align").as_deref() {
        Some("center") => Some(1),
        Some("right") => Some(2),
        _ => None,
    }
}

/// Wrap `emit` with `ESC a n` / `ESC a 0` if `align` is set; otherwise
/// just run `emit` unchanged. Used for the print-on-close elements
/// (image/barcode/symbol) which — unlike `<text>` — have no other reason
/// to track a "pending reset" across multiple events.
fn emit_aligned(out: &mut Vec<u8>, align: Option<u8>, emit: impl FnOnce(&mut Vec<u8>)) {
    if let Some(a) = align {
        out.extend_from_slice(&escpos::align(a));
    }
    emit(out);
    if align.is_some() {
        out.extend_from_slice(&escpos::align(0));
    }
}

/// `<barcode>`'s `hri` attribute (human-readable interpretation line):
/// none/above/below/both. Defaults to "above" per common Epson driver
/// behavior when the attribute is omitted.
fn parse_hri(e: &quick_xml::events::BytesStart<'_>) -> u8 {
    match attr_str(e, "hri").as_deref() {
        Some("none") => 0,
        Some("below") => 2,
        Some("both") => 3,
        _ => 1, // "above", and the default when unspecified
    }
}

/// Whether `<symbol type="...">` requests a QR code. Only QR is
/// implemented (`escpos::qr_code` uses the QR-specific `GS ( k`
/// sub-functions) — other 2D symbologies (pdf417, data_matrix, aztec,
/// gs1_databar_*, maxicode) are intentionally skipped rather than
/// sending QR-shaped bytes for a symbology the printer isn't expecting,
/// which could produce a printer error or garbage output.
fn is_qr_symbol_type(s: &str) -> bool {
    let s = s.to_lowercase();
    s.is_empty() || s.starts_with("qrcode")
}

fn apply_text_style(e: &quick_xml::events::BytesStart<'_>, out: &mut Vec<u8>) -> bool {
    let mut applied = false;
    if attr_str(e, "em").as_deref() == Some("true") {
        out.extend_from_slice(&escpos::emphasis(true));
        applied = true;
    }
    if attr_str(e, "ul").as_deref() == Some("true") {
        out.extend_from_slice(&escpos::underline(1));
        applied = true;
    }
    let width = attr_u8(e, "width");
    let height = attr_u8(e, "height");
    if width.is_some() || height.is_some() {
        out.extend_from_slice(&escpos::char_size(width.unwrap_or(1), height.unwrap_or(1)));
        applied = true;
    }
    applied
}

/// Undo whatever [`apply_text_style`] applied: emphasis off, underline
/// off, character size back to 1x1.
fn reset_text_style(out: &mut Vec<u8>) {
    out.extend_from_slice(&escpos::emphasis(false));
    out.extend_from_slice(&escpos::underline(0));
    out.extend_from_slice(&escpos::char_size(1, 1));
}

/// Best-effort validation that barcode `data` looks like valid content
/// for the given `escpos` barcode type code — purely defensive, catching
/// obviously malformed input (e.g. letters in a numeric-only barcode)
/// before it reaches the printer instead of silently sending it and
/// letting the printer reject or garble the barcode. Returns `true`
/// (and logs a warning) for anything not covered here rather than
/// blocking output the printer might still handle fine.
fn warn_if_invalid_barcode_data(btype: u8, data: &str) -> bool {
    // UPC-A, UPC-E, EAN13, EAN8, ITF are numeric-only per their specs.
    let numeric_only = matches!(btype, 0 | 1 | 2 | 3 | 5);
    if numeric_only && !data.bytes().all(|b| b.is_ascii_digit()) {
        warn!(target: "epos", "barcode data is not numeric for a numeric-only barcode type | btype={} data={:?}", btype, data);
        return false;
    }
    true
}

fn parse_barcode_type(s: &str) -> u8 {
    match s.to_uppercase().as_str() {
        "UPCA" => 0, "UPCE" => 1, "EAN13" => 2, "EAN8" => 3,
        "CODE39" => 4, "ITF" => 5, "CODABAR" => 6, "CODE93" => 7, "CODE128" => 8,
        _ => 8,
    }
}

fn parse_ecc_level(s: &str) -> u8 {
    match s.to_uppercase().as_str() {
        "L" => 1, "M" => 2, "Q" => 3, "H" => 4,
        _ => 3,
    }
}

fn attr_str(start: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    for attr in start.attributes().with_checks(false).flatten() {
        if attr.key.as_ref() == key.as_bytes() {
            return Some(String::from_utf8_lossy(&attr.value).into_owned());
        }
    }
    None
}

fn attr_u8(start: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<u8> {
    attr_str(start, key).and_then(|s| s.parse().ok())
}

fn attr_usize(start: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<usize> {
    attr_str(start, key).and_then(|s| s.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden() -> &'static [u8] {
        br#"<?xml version="1.0" encoding="utf-8"?>
<epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
  <text>Hello</text>
  <feedline/>
  <cut/>
</epos-print>"#
    }

    #[test]
    fn translates_text_feed_cut() {
        let out = translate(golden(), Options::default()).unwrap();
        assert_eq!(&out[..2], &[0x1B, b'@']);
        assert!(out.windows(5).any(|w| w == b"Hello"), "expected Hello in out: {:02x?}", out);
        assert!(out.windows(3).any(|w| w == &[0x1B, b'd', 1]), "expected Feed(1) in out: {:02x?}", out);
        assert!(out.windows(3).any(|w| w == &[0x1D, b'V', 1]), "expected PartialCut GS V 1 in out: {:02x?}", out);
    }

    /// Regression test for the "image printed nothing" bug: <image> base64
    /// raster data is element TEXT CONTENT per the Epson ePOS-Print XML
    /// spec, not a `data` attribute — this example is straight from the
    /// official manual (ePOS-Print XML User's Manual, "<image>", p.75):
    /// an 8x8 fully-filled-in raster image.
    #[test]
    fn image_element_text_content_is_decoded_as_raster() {
        let body = br#"<epos-print xmlns="x"><image width="8" height="8">//////////8=</image></epos-print>"#;
        let out = translate(body, Options { paper_width: 8, ..Default::default() }).unwrap();
        // One GS ( L fn112 (store) command for the whole image — width=8,
        // height=8, then 8 data bytes of 0xFF (one per row) — NOT split
        // into 8 separate single-row commands (that used to be slow and
        // added extra vertical gaps between rows), followed by fn50 (print).
        let expected: Vec<u8> = {
            let mut v = vec![escpos::GS, b'(', b'L', 18, 0, b'0', 112, b'0', 1, 1, b'1', 8, 0, 8, 0];
            v.extend_from_slice(&[0xFF; 8]);
            v.extend_from_slice(&[escpos::GS, b'(', b'L', 2, 0, b'0', 50]);
            v
        };
        assert!(
            out.windows(expected.len()).any(|w| w == expected.as_slice()),
            "expected single GS ( L raster command for all 8 rows in out: {:02x?}", out
        );
    }

    #[test]
    fn image_data_attribute_self_closing_still_works() {
        // Fallback path: some emitters may self-close with a `data` attribute.
        let body = br#"<epos-print xmlns="x"><image width="8" height="8" data="//////////8="/></epos-print>"#;
        let out = translate(body, Options { paper_width: 8, ..Default::default() }).unwrap();
        assert!(out.windows(3).any(|w| w == &[escpos::GS, b'(', b'L']), "expected raster command in out: {:02x?}", out);
    }

    #[test]
    fn strict_xml_rejects_unknown() {
        let body = br#"<epos-print xmlns="x"><unknown/></epos-print>"#;
        let err = translate(body, Options { strict_xml: true, ..Default::default() }).unwrap_err();
        assert!(matches!(err, TranslateError::UnknownElement(_)));
    }

    #[test]
    fn lenient_silently_skips_unknown() {
        let body = br#"<epos-print xmlns="x"><unknown/><text>x</text></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(1).any(|w| w == b"x"), "expected x in out: {:02x?}", out);
    }

    #[test]
    fn drawer_blocked_when_not_allowed() {
        let body = br#"<epos-print xmlns="x"><drawer/></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(!out.windows(3).any(|w| w == &[0x1B, b'p', 0]));
    }

    #[test]
    fn drawer_emitted_when_allowed() {
        let body = br#"<epos-print xmlns="x"><drawer/></epos-print>"#;
        let out = translate(body, Options { allow_drawer: true, ..Default::default() }).unwrap();
        assert!(out.windows(5).any(|w| w == &[0x1B, b'p', 0, 100, 250]), "expected Drawer in out: {:02x?}", out);
    }

    #[test]
    fn barcode_rejects_non_numeric_data_for_numeric_type() {
        // EAN13 is numeric-only; letters would previously be sent straight
        // to the printer as-is, risking a rejected/garbled barcode.
        let body = br#"<epos-print xmlns="x"><barcode type="ean13">abc123</barcode></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(!out.windows(3).any(|w| w == &[0x1D, b'k']), "expected no barcode command for invalid EAN13 data: {:02x?}", out);
    }

    #[test]
    fn barcode_allows_non_numeric_data_for_code128() {
        // CODE128 isn't numeric-only — must not be rejected by the same check.
        let body = br#"<epos-print xmlns="x"><barcode type="code128">ABC-123</barcode></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(2).any(|w| w == [0x1D, b'k']), "expected barcode command for CODE128: {:02x?}", out);
    }

    #[test]
    fn barcode_hri_none_is_respected() {
        let body = br#"<epos-print xmlns="x"><barcode type="code128" hri="none">12345</barcode></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(3).any(|w| w == &[0x1D, b'H', 0]), "expected GS H 0 (hri=none): {:02x?}", out);
    }

    #[test]
    fn barcode_hri_defaults_to_above() {
        let body = br#"<epos-print xmlns="x"><barcode type="code128">12345</barcode></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(3).any(|w| w == &[0x1D, b'H', 1]), "expected GS H 1 (default above): {:02x?}", out);
    }

    #[test]
    fn barcode_align_center_is_applied_and_reset() {
        let body = br#"<epos-print xmlns="x"><barcode type="code128" align="center">12345</barcode></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        let center_pos = out.windows(3).position(|w| w == [0x1B, b'a', 1]);
        let reset_pos = out.windows(3).position(|w| w == [0x1B, b'a', 0]);
        assert!(center_pos.is_some() && reset_pos.is_some() && reset_pos > center_pos, "got {:02x?}", out);
    }

    #[test]
    fn image_align_right_is_applied() {
        let body = br#"<epos-print xmlns="x"><image width="8" height="8" align="right">//////////8=</image></epos-print>"#;
        let out = translate(body, Options { paper_width: 8, ..Default::default() }).unwrap();
        assert!(out.windows(3).any(|w| w == &[0x1B, b'a', 2]), "expected ESC a 2 (right): {:02x?}", out);
    }

    #[test]
    fn symbol_non_qr_type_is_skipped_not_garbage() {
        // pdf417/data_matrix/etc aren't implemented — must be a safe no-op,
        // not QR-shaped bytes sent for a symbology the printer isn't
        // expecting (which risks a printer error or garbage output).
        let body = br#"<epos-print xmlns="x"><symbol type="pdf417">hello</symbol></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(!out.windows(3).any(|w| w == &[0x1D, b'(', b'k']), "expected no QR command for pdf417: {:02x?}", out);
    }

    #[test]
    fn symbol_qr_type_still_prints() {
        let body = br#"<epos-print xmlns="x"><symbol type="qrcode_model_2">hello</symbol></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(3).any(|w| w == &[0x1D, b'(', b'k']), "expected QR command: {:02x?}", out);
    }

    #[test]
    fn nested_text_line_content() {
        let body = br#"<epos-print xmlns="x"><text><line><content>Line 1</content></line><line><content>Line 2</content></line></text><cut/></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(6).any(|w| w == b"Line 1"), "got {:02x?}", out);
        assert!(out.windows(6).any(|w| w == b"Line 2"), "got {:02x?}", out);
        assert!(out.windows(3).any(|w| w == &[0x1D, b'V', 1]), "got {:02x?}", out);
    }

    /// Regression test: <text em="true" ul="true" width="2" height="3">
    /// used to silently drop all four attributes and print flat,
    /// unstyled text — escpos::emphasis/underline/char_size already
    /// existed but were never called from here.
    #[test]
    fn text_style_attributes_are_applied_and_reset() {
        let body = br#"<epos-print xmlns="x"><text em="true" ul="true" width="2" height="3">Bold</text></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(3).any(|w| w == &[0x1B, b'E', 1]), "expected emphasis on: {:02x?}", out);
        assert!(out.windows(3).any(|w| w == &[0x1B, b'-', 1]), "expected underline on: {:02x?}", out);
        // width=2 -> high nibble 0x10, height=3 -> low nibble 0x02 -> 0x12
        assert!(out.windows(3).any(|w| w == &[0x1D, b'!', 0x12]), "expected char_size(2,3): {:02x?}", out);
        assert!(out.windows(4).any(|w| w == b"Bold"), "expected text content: {:02x?}", out);
        // Reset back to defaults after the element closes.
        assert!(out.windows(3).any(|w| w == &[0x1B, b'E', 0]), "expected emphasis off after: {:02x?}", out);
        assert!(out.windows(3).any(|w| w == &[0x1B, b'-', 0]), "expected underline off after: {:02x?}", out);
        assert!(out.windows(3).any(|w| w == &[0x1D, b'!', 0x00]), "expected char_size reset after: {:02x?}", out);
    }

    #[test]
    fn text_without_style_attributes_emits_no_style_bytes() {
        let body = br#"<epos-print xmlns="x"><text>Plain</text></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(!out.windows(3).any(|w| w == &[0x1B, b'E', 1]));
        assert!(!out.windows(3).any(|w| w == &[0x1B, b'-', 1]));
        assert!(!out.windows(3).any(|w| w[0] == 0x1D && w[1] == b'!'));
    }

    /// End-to-end test against the exact payload Odoo POS sends when the user
    /// clicks "Test" on a Printer config (Setup > Printer > Test button).
    /// Fixture: testdata/odoo-pos-test-print.xml
    #[test]
    fn translates_odoo_pos_test_print() {
        const PAYLOAD: &[u8] = include_bytes!("../testdata/odoo-pos-test-print.xml");
        let out = translate(PAYLOAD, Options::default()).unwrap();

        // 1. Init at the start.
        assert_eq!(&out[..2], &[0x1B, b'@'], "init: {:02x?}", out);

        // 2. ESC a 1 (align center) for <text align="center">.
        assert!(
            out.windows(3).any(|w| w == &[0x1B, b'a', 1]),
            "expected ESC a 1 (center): {:02x?}", out
        );

        // 3. The text content includes the literal `&#10;` (newline) so we
        //    should see "Test print for printer Printer\n" in the output.
        let text = b"Test print for printer Printer\n";
        assert!(
            out.windows(text.len()).any(|w| w == text),
            "expected {:?} in out: {:02x?}", std::str::from_utf8(text).unwrap(), out
        );

        // 4. ESC d 1 (feed 1 line) from <feed line="1"/>.
        assert!(
            out.windows(3).any(|w| w == &[0x1B, b'd', 1]),
            "expected Feed(1): {:02x?}", out
        );

        // 5. ESC d 3 (feed 3 lines) from <feed line="3"/>.
        assert!(
            out.windows(3).any(|w| w == &[0x1B, b'd', 3]),
            "expected Feed(3): {:02x?}", out
        );

        // 6. GS V m=1 (PARTIAL cut, NOT GS V 0 = full cut).
        //    This was the bug: <cut type="feed"/> was emitting GS V 0 (full).
        assert!(
            out.windows(3).any(|w| w == &[0x1D, b'V', 1]),
            "expected PartialCut GS V 1: {:02x?}", out
        );
        assert!(
            !out.windows(3).any(|w| w == &[0x1D, b'V', 0]),
            "must NOT emit FullCut GS V 0 for type=feed: {:02x?}", out
        );

        // 7. ESC a 0 (align reset) must come AFTER the text.
        let center_pos = find_subseq(&out, &[0x1B, b'a', 1]).unwrap();
        let reset_pos = find_subseq(&out, &[0x1B, b'a', 0]).unwrap();
        assert!(reset_pos > center_pos, "align reset must come after align center");
    }

    #[test]
    fn cut_type_no_feed_emits_partial_only() {
        let body = br#"<epos-print xmlns="x"><cut type="no-feed"/></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(3).any(|w| w == &[0x1D, b'V', 1]));
        assert!(!out.windows(3).any(|w| w[0] == 0x1B && w[1] == b'd'));
    }

    #[test]
    fn cut_type_full_emits_gs_v_0() {
        let body = br#"<epos-print xmlns="x"><cut type="full"/></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(3).any(|w| w == &[0x1D, b'V', 0]));
    }

    #[test]
    fn cut_default_is_partial() {
        let body = br#"<epos-print xmlns="x"><cut/></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(3).any(|w| w == &[0x1D, b'V', 1]));
    }

    fn find_subseq(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }
}
