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

/// Default printer paper width in dots (576 = 80mm at 203dpi).
pub const DEFAULT_PAPER_WIDTH: usize = 576;

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub verbose: bool,
    pub allow_drawer: bool,
    pub strict_xml: bool,
    /// Printer's physical paper width in dots, used to size raster image
    /// bands (see `<image>` handling). Not related to any XML attribute —
    /// the ePOS-Print `<image>` element's own `width`/`height` attributes
    /// describe the image itself, in dots.
    pub paper_width: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            verbose: false,
            allow_drawer: false,
            strict_xml: false,
            paper_width: DEFAULT_PAPER_WIDTH,
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

    // State: which element context we are in. None means "at top level".
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Ctx {
        None,
        Text { align: Option<u8> },
        Line,    // inside <text> -> collecting <content> for the next line
        Content, // inside <line> -> collecting chardata for the current line
        Barcode { btype: u8, width: u8, height: u8, buf: Vec<u8> },
        Symbol { ecc: u8, buf: Vec<u8> },
        // <image width=".." height="..">base64-raster-data</image> — the
        // Epson spec (ePOS-Print XML User's Manual, "<image>") puts the
        // base64Binary raster data as element text content, not an
        // attribute. width/height here are the image's own dots.
        Image { width: usize, height: usize, buf: Vec<u8> },
    }
    let mut ctx = Ctx::None;
    let mut pending_text = String::new();
    let mut pending_align_reset = false;

    let mut dbg_count = 0;
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
                                out.extend_from_slice(&escpos::raster_banded(&img, width, height, opts.paper_width));
                            }
                        }
                    }
                    "barcode" => {
                        let btype = attr_str(&e, "type").as_deref().map(parse_barcode_type).unwrap_or(8);
                        let width = attr_u8(&e, "width").unwrap_or(3).clamp(2, 6);
                        let height = attr_u8(&e, "height").unwrap_or(100);
                        // For Empty, no inner chardata — use attribute only.
                        if let Some(data) = attr_str(&e, "data") {
                            let data = data.trim().to_string();
                            if !data.is_empty() {
                                out.extend_from_slice(&escpos::barcode(data.as_bytes(), btype, width, height));
                            }
                        }
                    }
                    "symbol" => {
                        let ecc = attr_str(&e, "eccLevel").as_deref().map(parse_ecc_level).unwrap_or(3);
                        if let Some(data) = attr_str(&e, "data") {
                            let data = data.trim().to_string();
                            if !data.is_empty() {
                                out.extend_from_slice(&escpos::qr_code(data.as_bytes(), ecc, 4));
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
                            buf: Vec::new(),
                        };
                    }
                    (Ctx::None, "symbol") => {
                        ctx = Ctx::Symbol {
                            ecc: attr_str(&e, "eccLevel").as_deref().map(parse_ecc_level).unwrap_or(3),
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
                            out.extend_from_slice(trimmed.as_bytes());
                        }
                        pending_text.clear();
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
                    if let Ctx::Barcode { btype, width, height, buf } = std::mem::replace(&mut ctx, Ctx::None) {
                        let data = String::from_utf8_lossy(&buf).trim().to_string();
                        if !data.is_empty() {
                            out.extend_from_slice(&escpos::barcode(data.as_bytes(), btype, width, height));
                        }
                    }
                } else if name == "symbol" {
                    if let Ctx::Symbol { ecc, buf } = std::mem::replace(&mut ctx, Ctx::None) {
                        let data = String::from_utf8_lossy(&buf).trim().to_string();
                        if !data.is_empty() {
                            out.extend_from_slice(&escpos::qr_code(data.as_bytes(), ecc, 4));
                        }
                    }
                } else if name == "image" {
                    if let Ctx::Image { width, height, buf } = std::mem::replace(&mut ctx, Ctx::None) {
                        let data = String::from_utf8_lossy(&buf);
                        let data = data.trim();
                        if !data.is_empty() {
                            let img = base64::engine::general_purpose::STANDARD.decode(data.as_bytes())?;
                            out.extend_from_slice(&escpos::raster_banded(&img, width, height, opts.paper_width));
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
        // GS v 0, xL=1, xH=0, yL=1, yH=0, then one data byte 0xFF, repeated
        // for 8 rows (height=8).
        let band = [escpos::GS, b'v', b'0', 1, 0, 1, 0, 0xFF];
        let count = out.windows(band.len()).filter(|w| *w == band).count();
        assert_eq!(count, 8, "expected 8 raster row bands in out: {:02x?}", out);
    }

    #[test]
    fn image_data_attribute_self_closing_still_works() {
        // Fallback path: some emitters may self-close with a `data` attribute.
        let body = br#"<epos-print xmlns="x"><image width="8" height="8" data="//////////8="/></epos-print>"#;
        let out = translate(body, Options { paper_width: 8, ..Default::default() }).unwrap();
        assert!(out.windows(3).any(|w| w == &[escpos::GS, b'v', b'0']), "expected raster command in out: {:02x?}", out);
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
    fn nested_text_line_content() {
        let body = br#"<epos-print xmlns="x"><text><line><content>Line 1</content></line><line><content>Line 2</content></line></text><cut/></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        assert!(out.windows(6).any(|w| w == b"Line 1"), "got {:02x?}", out);
        assert!(out.windows(6).any(|w| w == b"Line 2"), "got {:02x?}", out);
        assert!(out.windows(3).any(|w| w == &[0x1D, b'V', 1]), "got {:02x?}", out);
    }
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
