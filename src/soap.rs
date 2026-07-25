//! SOAP envelope parsing + response builders for the ePOS-Print protocol.
//!
//! Two request body shapes are accepted:
//! - **SOAP** — wrapped in `<s:Envelope>` / `<s:Body>`. Some Epson SDK clients.
//! - **Raw** — bare `<epos-print>...</epos-print>`. What real Odoo POS sends.
//!
//! Responses mirror the request shape: raw in -> bare `<response/>`, SOAP in ->
//! wrapped envelope. Both success and error variants are provided.

use std::fmt;

/// Request body shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Unknown,
    Soap,
    Raw,
}

/// Success/error selection for response builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Success,
    Error,
}

/// Detect whether `data` is a SOAP envelope or a raw `<epos-print>` document.
pub fn detect_format(data: &[u8]) -> Format {
    let s = std::str::from_utf8(data).unwrap_or("");
    if s.contains("<s:Envelope")
        || s.contains("<SOAP-ENV:Envelope")
        || s.contains(":Envelope ")
        || s.contains("<Envelope ")
    {
        return Format::Soap;
    }
    if s.contains("<epos-print") {
        return Format::Raw;
    }
    Format::Unknown
}

/// AC4-golden SOAP success response (matches byte-for-byte).
pub const SOAP_SUCCESS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="true" code="" status="252" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>"#;

/// Bare XML success response, matching the odoo/epos-proxy wire format.
pub const RAW_SUCCESS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<response success="true" code="" status="" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>"#;

/// Build a SOAP envelope error response with the given code.
pub fn soap_error(code: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="false" code="{code}" status="0" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>"#
    )
}

/// Build a bare XML error response.
pub fn raw_error(code: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<response success="false" code="{code}" status="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>"#
    )
}

/// Extract the epos-print inner content from a request body.
/// Returns the inner XML between `<epos-print ...>` and `</epos-print>`,
/// or an error if no epos-print element is found.
pub fn parse(data: &[u8]) -> Result<String, ParseError> {
    extract(data).ok_or(ParseError::NotFound)
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("no epos-print content found")]
    NotFound,
}

fn strip_self_close(s: &str) -> String {
    s.trim_end_matches("/>").to_string()
}

fn extract(data: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(data).ok()?;
    let start = s.find("<epos-print")?;
    let after = &s[start..];
    let tag_end = after.find('>')?;
    let content_start = start + tag_end + 1;
    let content = &s[content_start..];
    if let Some(end) = content.find("</epos-print>") {
        return Some(content[..end].to_string());
    }
    Some(content.to_string())
}

/// Convenience: render the response bytes for `(format, kind, code)`.
pub fn render(format: Format, kind: Kind, code: &str) -> Vec<u8> {
    let s = match (format, kind) {
        (Format::Raw, Kind::Success) => RAW_SUCCESS.to_string(),
        (Format::Raw, Kind::Error) => raw_error(code),
        _ if matches!(kind, Kind::Success) => SOAP_SUCCESS.to_string(),
        _ => soap_error(code),
    };
    s.into_bytes()
}

/// Display format name for log messages.
impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Soap => write!(f, "soap"),
            Format::Raw => write!(f, "raw"),
            Format::Unknown => write!(f, "unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW_REQUEST: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?>
<epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
  <text>Hello</text>
  <cut/>
</epos-print>"#;

    const SOAP_REQUEST: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
      <text>Hello</text>
    </epos-print>
  </s:Body>
</s:Envelope>"#;

    #[test]
    fn detect_raw() {
        assert_eq!(detect_format(RAW_REQUEST), Format::Raw);
    }

    #[test]
    fn detect_soap() {
        assert_eq!(detect_format(SOAP_REQUEST), Format::Soap);
    }

    #[test]
    fn detect_unknown() {
        assert_eq!(detect_format(b"hello world"), Format::Unknown);
    }

    #[test]
    fn parse_extracts_inner_xml() {
        let inner = parse(RAW_REQUEST).expect("parse raw");
        assert!(inner.contains("<text>"));
        assert!(inner.contains("<cut"));
    }

    #[test]
    fn parse_handles_self_closing() {
        let body = br#"<epos-print xmlns="x"><cut/></epos-print>"#;
        let inner = parse(body).expect("parse");
        assert_eq!(inner, "<cut/>");
    }

    #[test]
    fn parse_missing_returns_error() {
        assert!(parse(b"<foo/>").is_err());
    }

    #[test]
    fn render_raw_success() {
        let out = render(Format::Raw, Kind::Success, "");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains(r#"success="true""#));
        assert!(!s.contains("Envelope")); // bare XML, no envelope wrapper
    }

    #[test]
    fn render_raw_error() {
        let out = render(Format::Raw, Kind::Error, "EX_BAD");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains(r#"success="false""#));
        assert!(s.contains(r#"code="EX_BAD""#));
    }

    #[test]
    fn render_soap_success_byte_for_byte() {
        let out = render(Format::Soap, Kind::Success, "");
        assert_eq!(std::str::from_utf8(&out).unwrap(), SOAP_SUCCESS);
    }
}
