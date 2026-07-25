//! Translate ePOS-Print XML into an ESC/POS byte stream.
//!
//! Supports the same element set as the Go implementation:
//! - `<text>` (with optional `align` attribute, flat chardata or nested
//!   `<line><content>` pattern)
//! - `<feedline>` / `<feed line="N">`
//! - `<cut/>`
//! - `<image data="..." width="N" height="N">` (1-bit raster, base64-encoded)
//! - `<barcode>` (UPC-A/B, EAN13/8, CODE39, ITF, CODABAR, CODE93, CODE128)
//! - `<symbol>` (QR code with ECC level)
//! - `<drawer>` (single pin, gated by `--drawer`)
//! - `<pulse>` (odoo/epos-proxy style, both pins)
//!
//! Unknown elements are silently skipped unless `strict_xml` is set, in which
//! case they produce [`TranslateError::UnknownElement`].

use base64::Engine;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::escpos;
use crate::soap;

/// Translation options.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub verbose: bool,
    pub allow_drawer: bool,
    pub strict_xml: bool,
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
}

/// Translate a request body (raw or SOAP-wrapped) into ESC/POS bytes.
pub fn translate(data: &[u8], opts: Options) -> Result<Vec<u8>, TranslateError> {
    let body = soap::parse(data)?;
    translate_inner(body.as_bytes(), opts)
}

fn translate_inner(body: &[u8], opts: Options) -> Result<Vec<u8>, TranslateError> {
    // Wrap in <root> so XML is well-formed even if the epos-print body
    // is a single self-closing element.
    let mut wrapped = Vec::with_capacity(body.len() + 13);
    wrapped.extend_from_slice(b"<root>");
    wrapped.extend_from_slice(body);
    wrapped.extend_from_slice(b"</root>");
    let mut reader = Reader::from_reader(&wrapped[..]);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    out.extend_from_slice(&escpos::init());

    let mut wrapper_depth: i32 = 0;
    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(TranslateError::Xml(format!("{e} at pos {}", reader.buffer_position()))),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                match name.as_str() {
                    "epos-print" | "root" | "parameter" => {
                        wrapper_depth += 1;
                    }
                    "text" => {
                        handle_text(&mut reader, &e, &mut out)?;
                    }
                    "feedline" => out.extend_from_slice(&escpos::feed(1)),
                    "feed" => {
                        let n = attr_u8(&e, "line").unwrap_or(1);
                        out.extend_from_slice(&escpos::feed(n));
                    }
                    "cut" => out.extend_from_slice(&escpos::cut(0)),
                    "image" => handle_image(&e, &mut out)?,
                    "drawer" => {
                        if opts.allow_drawer {
                            out.extend_from_slice(&escpos::drawer(100, 250));
                        }
                    }
                    "barcode" => handle_barcode(&mut reader, &e, &mut out)?,
                    "symbol" => handle_symbol(&mut reader, &e, &mut out)?,
                    "pulse" => {
                        if opts.allow_drawer {
                            out.extend_from_slice(&escpos::pulse());
                        }
                    }
                    other => {
                        if opts.strict_xml {
                            return Err(TranslateError::UnknownElement(other.to_string()));
                        }
                        // Skip the subtree; quick-xml advances naturally.
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "epos-print" {
                    wrapper_depth -= 1;
                    if wrapper_depth < 0 {
                        break;
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn handle_text(
    reader: &mut Reader<&[u8]>,
    _start: &quick_xml::events::BytesStart<'_>,
    out: &mut Vec<u8>,
) -> Result<(), TranslateError> {
    let _ = _start;
    let align = match attr_str(_start, "align").as_deref() {
        Some("center") => Some(1u8),
        Some("right") => Some(2u8),
        _ => None,
    };
    if let Some(a) = align {
        out.extend_from_slice(&escpos::align(a));
    }

    // Read until matching End. Capture inner content as bytes (handles both
    // `<text>chardata</text>` and `<text><line><content>x</content></line></text>`).
    let mut depth = 1;
    let mut text_buf = Vec::new();
    let mut local_buf: Vec<u8> = Vec::new();
    loop {
        local_buf.clear();
        match reader.read_event_into(&mut local_buf) {
            Err(e) => return Err(TranslateError::Xml(format!("{e} at text pos {}", reader.buffer_position()))),
            Ok(Event::Eof) => break,
            Ok(Event::Start(_)) => {
                depth += 1;
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Text(t)) => {
                text_buf.extend_from_slice(t.unescape().unwrap_or_default().as_bytes());
                text_buf.push(b'\n');
            }
            _ => {}
        }
    }
    let trimmed = String::from_utf8_lossy(&text_buf).trim().to_string();
    if !trimmed.is_empty() {
        out.extend_from_slice(trimmed.as_bytes());
    }
    if let Some(a) = align {
        out.extend_from_slice(&escpos::align(0));
    }
    Ok(())
}

fn handle_image(
    start: &quick_xml::events::BytesStart<'_>,
    out: &mut Vec<u8>,
) -> Result<(), TranslateError> {
    let data = attr_str(start, "data").unwrap_or_default();
    if data.is_empty() {
        return Ok(());
    }
    let mut paper_width = attr_usize(start, "width").unwrap_or(0);
    let height = attr_usize(start, "height").unwrap_or(1);
    let width = attr_usize(start, "x").unwrap_or(0);
    if paper_width == 0 {
        paper_width = 576;
    }
    let width = if width == 0 { paper_width } else { width };
    let img = base64::engine::general_purpose::STANDARD.decode(data.as_bytes())?;
    out.extend_from_slice(&escpos::raster_banded(&img, width, height.max(1), paper_width));
    Ok(())
}

fn handle_barcode(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    out: &mut Vec<u8>,
) -> Result<(), TranslateError> {
    let mut local_buf: Vec<u8> = Vec::new();
    let mut barcode_type = attr_str(start, "type").map(|s| parse_barcode_type(&s)).unwrap_or(8);
    let mut width = attr_u8(start, "width").unwrap_or(3).clamp(2, 6);
    let mut height = attr_u8(start, "height").unwrap_or(100);
    let mut data = attr_str(start, "data").unwrap_or_default();
    if data.is_empty() {
        // data may be inside element chardata instead of attribute
        let inner = read_inner_text(reader, start)?;
        data = inner.trim().to_string();
    }
    if data.is_empty() {
        return Ok(());
    }
    let _ = (&mut barcode_type, &mut width, &mut height);
    out.extend_from_slice(&escpos::barcode(data.as_bytes(), barcode_type, width, height));
    Ok(())
}

fn handle_symbol(
    reader: &mut Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    out: &mut Vec<u8>,
) -> Result<(), TranslateError> {
    let mut local_buf: Vec<u8> = Vec::new();
    let mut ecc_level = attr_str(start, "eccLevel").map(|s| parse_ecc_level(&s) as u8).unwrap_or(3);
    let mut data = attr_str(start, "data").unwrap_or_default();
    if data.is_empty() {
        let inner = read_inner_text(reader, start)?;
        data = inner.trim().to_string();
    }
    if data.is_empty() {
        return Ok(());
    }
    let _ = &mut ecc_level;
    out.extend_from_slice(&escpos::qr_code(data.as_bytes(), ecc_level, 4));
    Ok(())
}

fn read_inner_text(
    reader: &mut Reader<&[u8]>,
    _start: &quick_xml::events::BytesStart<'_>,
) -> Result<String, TranslateError> {
    let mut local_buf: Vec<u8> = Vec::new();
    // Skip past the start tag, then collect text until matching end.
    let mut depth = 1;
    let mut out = Vec::new();
    loop {
        local_buf.clear();
        match reader.read_event_into(&mut local_buf) {
            Err(e) => return Err(TranslateError::Xml(format!("{e}"))),
            Ok(Event::Eof) => break,
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(e)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Text(t)) => {
                out.extend_from_slice(t.unescape().unwrap_or_default().as_bytes());
            }
            _ => {}
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn parse_barcode_type(s: &str) -> u8 {
    match s.to_uppercase().as_str() {
        "UPCA" => 0,
        "UPCE" => 1,
        "EAN13" => 2,
        "EAN8" => 3,
        "CODE39" => 4,
        "ITF" => 5,
        "CODABAR" => 6,
        "CODE93" => 7,
        "CODE128" => 8,
        _ => 8,
    }
}

fn parse_ecc_level(s: &str) -> u8 {
    match s.to_uppercase().as_str() {
        "L" => 1,
        "M" => 2,
        "Q" => 3,
        "H" => 4,
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
        assert!(out.windows(5).any(|w| w == b"Hello"));
        assert!(out.windows(3).any(|w| w == &[0x1B, b'd', 1]));
        assert!(out.windows(3).any(|w| w == &[0x1D, b'V', 0]));
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
        assert!(out.windows(1).any(|w| w == b"x"));
    }

    #[test]
    fn drawer_blocked_when_not_allowed() {
        let body = br#"<epos-print xmlns="x"><drawer/></epos-print>"#;
        let out = translate(body, Options::default()).unwrap();
        // ESC p sequence should be absent
        assert!(!out.windows(3).any(|w| w == &[0x1B, b'p', 0]));
    }

    #[test]
    fn drawer_emitted_when_allowed() {
        let body = br#"<epos-print xmlns="x"><drawer/></epos-print>"#;
        let out = translate(body, Options { allow_drawer: true, ..Default::default() }).unwrap();
        assert!(out.windows(5).any(|w| w == &[0x1B, b'p', 0, 100, 250]));
    }
}
