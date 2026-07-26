//! ESC/POS command bytes.
//!
//! Each function returns a fresh `Vec<u8>` that callers may append into a
//! larger buffer (typically produced by [`translate::translate`]).

pub const ESC: u8 = 0x1B;
pub const GS: u8 = 0x1D;
pub const CRLF: &[u8] = &[0x0D, 0x0A];

/// ESC @ — initialize printer.
pub fn init() -> Vec<u8> {
    vec![ESC, b'@']
}

/// GS V m — cut paper. `m = 0` full cut, `m = 1` partial.
pub fn cut(m: u8) -> Vec<u8> {
    vec![GS, b'V', m]
}

/// ESC d n — feed n lines.
pub fn feed(n: u8) -> Vec<u8> {
    vec![ESC, b'd', n]
}

/// Raw text bytes (no encoding applied). Caller is responsible for codepage.
pub fn text(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// ESC a n — text alignment. `n = 0` left, `1` center, `2` right.
pub fn align(n: u8) -> Vec<u8> {
    vec![ESC, b'a', n]
}

/// ESC E n — emphasized mode.
pub fn emphasis(on: bool) -> Vec<u8> {
    vec![ESC, b'E', u8::from(on)]
}

/// ESC - n — underline.
pub fn underline(n: u8) -> Vec<u8> {
    vec![ESC, b'-', n]
}

/// GS ! n — character size. Bit 4 (0x10) = double height, bit 5 (0x20) = double width.
pub fn double_height(on: bool) -> Vec<u8> {
    vec![GS, b'!', if on { 0x10 } else { 0x00 }]
}

/// GS ( L — print a raster image via Function 112 (store graphics data)
/// followed by Function 50 (print the graphics data in the print buffer).
/// This is Epson's documented replacement for the obsolete `GS v 0` raster
/// command; `GS v 0` remaining in single-command form was still slow on
/// real hardware because raster mode throttles the print engine, and
/// `GS ( L` is what real Epson network printers and mature ESC/POS
/// libraries (e.g. python-escpos) use for fast graphics printing.
///
/// Byte layout, per Epson ESC/POS "GS ( L <Function 112/50>":
///   `1D 28 4C pL pH m fn tone xm ym colors xL xH yL yH d[0]..d[k]`
///   followed by `1D 28 4C 02 00 m fn` (fn=50, no payload) to trigger
///   the actual print.
/// - `pL pH`: little-endian byte count of everything from `m` onward.
/// - `m`: fixed 0x30 for both functions.
/// - `fn`: 112 (store) then 50 (print).
/// - `tone`: 0x30 = monochrome. `colors`: 0x31 = single color.
/// - `xm`/`ym`: dot pitch, 1 = one physical dot per source pixel (the
///   density that matches our existing 1-pixel-per-dot image data;
///   2 would double each pixel into a 2x2 dot block).
/// - `xL xH yL yH`: image width/height in **pixels** (not bytes).
/// - `d[0]..d[k]`: 1bpp raster, row-major, MSB = leftmost pixel, each row
///   zero-padded to a byte boundary — same packing as `GS v 0`.
///
/// `img` is row-major, `(width + 7) / 8` bytes per row, MSB = leftmost
/// pixel. `paper_width` is the printer's paper width in dots (576 = 80mm,
/// 384 = 58mm); output rows are padded/clipped to it.
pub fn raster_print(img: &[u8], width: usize, height: usize, paper_width: usize) -> Vec<u8> {
    if img.is_empty() || width == 0 || height == 0 {
        return Vec::new();
    }
    let dst_bytes_per_row = paper_width.div_ceil(8);
    let src_bytes_per_row = width.div_ceil(8);
    let print_width = width.min(paper_width);

    let mut raster = vec![0u8; dst_bytes_per_row * height];
    for y in 0..height {
        let src_off = y * src_bytes_per_row;
        let dst_off = y * dst_bytes_per_row;
        for x in 0..print_width {
            let byte_idx = src_off + x / 8;
            let bit_idx = 7 - (x % 8) as u32;
            if byte_idx < img.len() && (img[byte_idx] >> bit_idx) & 1 == 1 {
                raster[dst_off + x / 8] |= 1 << bit_idx;
            }
        }
    }

    let width_dots = print_width.min(u16::MAX as usize) as u16;
    let height_dots = height.min(u16::MAX as usize) as u16;

    let mut payload = Vec::with_capacity(8 + raster.len());
    payload.push(b'0'); // tone: monochrome
    payload.push(1); // xm: normal density (1 dot per source pixel)
    payload.push(1); // ym: normal density (1 dot per source pixel)
    payload.push(b'1'); // colors: single color
    payload.extend_from_slice(&width_dots.to_le_bytes());
    payload.extend_from_slice(&height_dots.to_le_bytes());
    payload.extend_from_slice(&raster);

    let mut out = Vec::with_capacity(payload.len() + 16);
    push_gs_paren_l(&mut out, 112, &payload); // store graphics data
    push_gs_paren_l(&mut out, 50, &[]); // print the buffered graphics data
    out
}

/// `GS ( L pL pH m fn data` framing shared by all Function 112/50 calls.
/// `m` is fixed at 0x30 for these two functions.
fn push_gs_paren_l(out: &mut Vec<u8>, func: u8, data: &[u8]) {
    let len = data.len() + 2; // + m byte + fn byte
    out.extend_from_slice(&[GS, b'(', b'L']);
    out.push((len % 256) as u8);
    out.push((len / 256) as u8);
    out.push(b'0'); // m
    out.push(func);
    out.extend_from_slice(data);
}

/// ESC p 0 t1 t2 — pulse drawer kick on pin 2.
pub fn drawer(t1: u8, t2: u8) -> Vec<u8> {
    vec![ESC, b'p', 0, t1, t2]
}

/// odoo/epos-proxy drawer pulse: ESC = 0x01 then ESC p 0 25 25 (pin 2)
/// and ESC p 1 25 25 (pin 5). Always emitted; not gated by `--drawer`.
pub fn pulse() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[ESC, b'=', 0x01]); // enable pulse
    out.extend_from_slice(&drawer(25, 25));   // pin 2
    out.extend_from_slice(&[ESC, b'p', 1, 25, 25]); // pin 5
    out
}

/// GS k m d1..dk NUL — print barcode.
/// `barcode_type` is the function code (65-86 per ESC/POS spec).
/// `width` = module width (2-6), `height` = bar height in dots (0-255).
pub fn barcode(data: &[u8], barcode_type: u8, width: u8, height: u8) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[GS, b'h', height]); // height
    out.extend_from_slice(&[GS, b'w', width]); // module width
    out.extend_from_slice(&[GS, b'H', 1]); // HRI above
    out.extend_from_slice(&[GS, b'k', barcode_type]);
    out.extend_from_slice(data);
    out.push(0); // NUL terminator
    out
}

/// GS ( k — QR code.
/// `ecc_level` is the error correction level (1=L, 2=M, 3=Q, 4=H).
/// `module_size` is the module size in dots (1-16).
pub fn qr_code(data: &[u8], ecc_level: u8, module_size: u8) -> Vec<u8> {
    let mut out = Vec::new();
    // Set module size
    out.extend_from_slice(&[GS, b'(', b'k', 4, 0, 16, module_size]);
    // Set ECC level
    out.extend_from_slice(&[GS, b'(', b'k', 3, 0, 49, ecc_level]);
    // Store data
    let len = data.len();
    out.extend_from_slice(&[GS, b'(', b'k', (len % 256) as u8, (len / 256) as u8, 49]);
    out.extend_from_slice(data);
    // Print
    out.extend_from_slice(&[GS, b'(', b'k', 2, 0, 48]);
    out
}

/// Minimal selftest sequence: init + header + feed + full cut.
pub fn self_test_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&init());
    out.extend_from_slice(&text("ePOS Emulator Self-Test\n"));
    out.extend_from_slice(&feed(2));
    out.extend_from_slice(&cut(0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_emits_esc_at() {
        assert_eq!(init(), vec![0x1B, b'@']);
    }

    #[test]
    fn cut_full_vs_partial() {
        assert_eq!(cut(0), vec![0x1D, b'V', 0]);
        assert_eq!(cut(1), vec![0x1D, b'V', 1]);
    }

    #[test]
    fn feed_count() {
        assert_eq!(feed(1), vec![0x1B, b'd', 1]);
        assert_eq!(feed(3), vec![0x1B, b'd', 3]);
    }

    #[test]
    fn text_passthrough() {
        assert_eq!(text("hello"), b"hello".to_vec());
    }

    #[test]
    fn drawer_emits_5_bytes() {
        assert_eq!(drawer(100, 250), vec![0x1B, b'p', 0, 100, 250]);
    }

    #[test]
    fn raster_single_row_paper_width_8() {
        // 1 row of 8 black pixels at 8-dot paper width → GS ( L fn112
        // (store: tone='0' xm=1 ym=1 colors='1' width=8 height=1 then the
        // 1-byte raster) followed by fn50 (print, no payload).
        let img = vec![0xFF];
        let out = raster_print(&img, 8, 1, 8);
        assert_eq!(
            out,
            vec![
                0x1D, b'(', b'L', 11, 0, b'0', 112,
                b'0', 1, 1, b'1', 8, 0, 1, 0, 0xFF,
                0x1D, b'(', b'L', 2, 0, b'0', 50,
            ]
        );
    }

    #[test]
    fn raster_multi_row_single_command() {
        // 2 rows on a 16-dot paper, each row is 2 bytes of 0xFF. Must be
        // ONE fn112 store command covering all rows (yL/height=2), not
        // per-row commands — GS v 0 called once per row used to turn a
        // continuous image print into hundreds of separate print-engine
        // feed cycles, which was both very slow and stacked up extra
        // vertical gaps between rows.
        let img = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let out = raster_print(&img, 16, 2, 16);
        assert_eq!(
            out,
            vec![
                0x1D, b'(', b'L', 14, 0, b'0', 112,
                b'0', 1, 1, b'1', 16, 0, 2, 0, 0xFF, 0xFF, 0xFF, 0xFF,
                0x1D, b'(', b'L', 2, 0, b'0', 50,
            ]
        );
    }

    #[test]
    fn self_test_includes_init_and_cut() {
        let out = self_test_bytes();
        assert_eq!(&out[..2], &[0x1B, b'@']);
        assert!(out.windows(3).any(|w| w == &[0x1D, b'V', 0]));
        assert!(out.windows(4).any(|w| w == b"Test" || w == b"Self"));
    }
}
