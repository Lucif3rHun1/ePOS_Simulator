//! Single-byte codepage encoding for ESC/POS thermal printers.
//!
//! Thermal printers interpret bytes >= 0x80 according to their currently
//! selected character table (`ESC t n`), not UTF-8/Unicode — sending raw
//! UTF-8 multi-byte sequences (which this project did before) prints
//! garbage for any accented letter, currency symbol, or curly quote.
//! Text must be transcoded to the printer's configured codepage first,
//! and the printer must be told (via `ESC t n`) which table to use.

use oem_cp::{Cp437, Cp850, Cp852, Cp858, Cp860, Cp863, Cp865, Cp866, StrExt};

/// Encode `text` for the named codepage (case-insensitive; accepts
/// "CP437", "437", "PC437", "windows-1252", "1252", etc). Unrecognized
/// names fall back to CP437, the power-on default on virtually all
/// ESC/POS thermal printers. Characters with no representation in the
/// target codepage become `?` (0x3F) rather than corrupting the byte
/// stream.
pub fn encode(text: &str, codepage: &str) -> Vec<u8> {
    match normalize(codepage).as_str() {
        "850" => text.to_cp_lossy::<Cp850>().into_iter().map(|c| c.0).collect(),
        "852" => text.to_cp_lossy::<Cp852>().into_iter().map(|c| c.0).collect(),
        "858" => text.to_cp_lossy::<Cp858>().into_iter().map(|c| c.0).collect(),
        "860" => text.to_cp_lossy::<Cp860>().into_iter().map(|c| c.0).collect(),
        "863" => text.to_cp_lossy::<Cp863>().into_iter().map(|c| c.0).collect(),
        "865" => text.to_cp_lossy::<Cp865>().into_iter().map(|c| c.0).collect(),
        "866" => text.to_cp_lossy::<Cp866>().into_iter().map(|c| c.0).collect(),
        "1252" => encode_windows_1252(text),
        _ => text.to_cp_lossy::<Cp437>().into_iter().map(|c| c.0).collect(),
    }
}

/// `ESC t n` table number for the named codepage, per the Epson ESC/POS
/// "Character Code Tables" reference. Must match [`encode`]'s mapping —
/// selecting the wrong table on the printer defeats correct encoding.
pub fn esc_t_table_number(codepage: &str) -> u8 {
    match normalize(codepage).as_str() {
        "850" => 2,
        "860" => 3,
        "863" => 4,
        "865" => 5,
        "1252" => 16,
        "866" => 17,
        "852" => 18,
        "858" => 19,
        _ => 0, // PC437
    }
}

fn normalize(name: &str) -> String {
    let s = name.trim().to_uppercase();
    for prefix in ["WINDOWS-", "WINDOWS", "CP", "PC"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    s
}

/// Windows-1252: identical to ASCII below 0x80 and to Latin-1 (direct
/// Unicode code point) from 0xA0 up; the 0x80-0x9F range has its own
/// fixed set of characters (curly quotes, dashes, €, etc) that differ
/// from the C1 control codes Latin-1 has there.
fn encode_windows_1252(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            let cp = c as u32;
            if cp < 0x80 || (0xA0..=0xFF).contains(&cp) {
                cp as u8
            } else {
                windows_1252_high_range(c).unwrap_or(b'?')
            }
        })
        .collect()
}

fn windows_1252_high_range(c: char) -> Option<u8> {
    Some(match c {
        '\u{20AC}' => 0x80,
        '\u{201A}' => 0x82,
        '\u{0192}' => 0x83,
        '\u{201E}' => 0x84,
        '\u{2026}' => 0x85,
        '\u{2020}' => 0x86,
        '\u{2021}' => 0x87,
        '\u{02C6}' => 0x88,
        '\u{2030}' => 0x89,
        '\u{0160}' => 0x8A,
        '\u{2039}' => 0x8B,
        '\u{0152}' => 0x8C,
        '\u{017D}' => 0x8E,
        '\u{2018}' => 0x91,
        '\u{2019}' => 0x92,
        '\u{201C}' => 0x93,
        '\u{201D}' => 0x94,
        '\u{2022}' => 0x95,
        '\u{2013}' => 0x96,
        '\u{2014}' => 0x97,
        '\u{02DC}' => 0x98,
        '\u{2122}' => 0x99,
        '\u{0161}' => 0x9A,
        '\u{203A}' => 0x9B,
        '\u{0153}' => 0x9C,
        '\u{017E}' => 0x9E,
        '\u{0178}' => 0x9F,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp437_ascii_passthrough() {
        assert_eq!(encode("Hello", "CP437"), b"Hello".to_vec());
    }

    #[test]
    fn cp437_accented_char_encodes_to_high_byte() {
        // 'é' is 0x82 in CP437.
        assert_eq!(encode("café", "CP437"), vec![b'c', b'a', b'f', 0x82]);
    }

    #[test]
    fn cp437_unmappable_char_becomes_question_mark() {
        // CJK characters have no CP437 representation.
        assert_eq!(encode("日本語", "CP437"), vec![b'?', b'?', b'?']);
    }

    #[test]
    fn windows_1252_euro_sign() {
        assert_eq!(encode("\u{20AC}10", "windows-1252"), vec![0x80, b'1', b'0']);
    }

    #[test]
    fn windows_1252_curly_quotes_and_dash() {
        assert_eq!(
            encode("\u{2018}hi\u{2019}\u{2014}", "1252"),
            vec![0x91, b'h', b'i', 0x92, 0x97]
        );
    }

    #[test]
    fn windows_1252_latin1_passthrough() {
        // 'é' (U+00E9) is 0xE9 in both Unicode and Windows-1252.
        assert_eq!(encode("café", "1252"), vec![b'c', b'a', b'f', 0xE9]);
    }

    #[test]
    fn unknown_codepage_falls_back_to_cp437() {
        assert_eq!(encode("café", "bogus"), vec![b'c', b'a', b'f', 0x82]);
    }

    #[test]
    fn table_numbers_match_epson_reference() {
        assert_eq!(esc_t_table_number("CP437"), 0);
        assert_eq!(esc_t_table_number("CP850"), 2);
        assert_eq!(esc_t_table_number("CP860"), 3);
        assert_eq!(esc_t_table_number("CP863"), 4);
        assert_eq!(esc_t_table_number("CP865"), 5);
        assert_eq!(esc_t_table_number("windows-1252"), 16);
        assert_eq!(esc_t_table_number("CP866"), 17);
        assert_eq!(esc_t_table_number("CP852"), 18);
        assert_eq!(esc_t_table_number("CP858"), 19);
    }
}
