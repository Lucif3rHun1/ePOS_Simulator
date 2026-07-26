//! Windows print spooler (winspool.drv) wrapper for RAW printing.
//!
//! FFI bindings are written directly here rather than going through windows-sys
//! because windows-sys 0.59's transitive `windows-targets` 0.52 doesn't include
//! the winspool link directive, which would force an extra crate dep just to
//! emit `rustc-link-lib=winspool`. Direct FFI is simpler and version-stable.

#[cfg(windows)]
mod imp {
    use std::ptr;

    use thiserror::Error;

    // ---- types ------------------------------------------------------------

    #[repr(C)]
    pub struct PRINTER_DEFAULTSW {
        pub pDatatype: *mut u16,
        pub pDevMode: *mut core::ffi::c_void,
        pub DesiredAccess: u32,
    }

    #[repr(C)]
    pub struct DOC_INFO_1W {
        pub pDocName: *mut u16,
        pub pOutputFile: *mut u16,
        pub pDatatype: *mut u16,
    }

    /// `PRINTER_ACCESS_USE` (0x00000008) — opens the printer for raw writes.
    pub const PRINTER_ACCESS_USE: u32 = 0x00000008;

    pub type Handle = *mut core::ffi::c_void;

    // ---- raw FFI ----------------------------------------------------------

    // The print spooler DLL is `winspool.drv` (not `winspool.dll`) — the
    // `+verbatim` modifier stops rustc from appending its default `.dll`
    // suffix, which otherwise produces an import for a nonexistent
    // `winspool.dll` and fails to load at runtime (STATUS_DLL_NOT_FOUND).
    #[repr(C)]
    pub struct PRINTER_INFO_5W {
        pub pPrinterName: *mut u16,
        pub pPortName: *mut u16,
        pub Attributes: u32,
        pub DeviceNotSelectedTimeout: u32,
        pub TransmissionRetryTimeout: u32,
    }

    /// Enumerate printers installed locally + persistent user connections,
    /// per MS docs' recommended flag pair for Level 5 (the cheap level:
    /// no per-printer RPC round-trip).
    const PRINTER_ENUM_LOCAL: u32 = 0x00000002;
    const PRINTER_ENUM_CONNECTIONS: u32 = 0x00000004;

    #[link(name = "winspool.drv", kind = "raw-dylib", modifiers = "+verbatim")]
    extern "system" {
        fn OpenPrinterW(
            pPrinterName: *const u16,
            phPrinter: *mut Handle,
            pDefault: *const PRINTER_DEFAULTSW,
        ) -> i32;
        fn ClosePrinter(hPrinter: Handle) -> i32;
        fn StartDocPrinterW(hPrinter: Handle, level: u32, pDocInfo: *const DOC_INFO_1W) -> u32;
        fn EndDocPrinter(hPrinter: Handle) -> i32;
        fn WritePrinter(
            hPrinter: Handle,
            pBuf: *const u8,
            cbBuf: u32,
            pcWritten: *mut u32,
        ) -> i32;
        fn EnumPrintersW(
            Flags: u32,
            Name: *const u16,
            Level: u32,
            pPrinterEnum: *mut u8,
            cbBuf: u32,
            pcbNeeded: *mut u32,
            pcReturned: *mut u32,
        ) -> i32;
        fn GetDefaultPrinterW(pszBuffer: *mut u16, pcchBuffer: *mut u32) -> i32;
    }

    #[link(name = "kernel32", kind = "raw-dylib")]
    extern "system" {
        fn GetLastError() -> u32;
    }

    // ---- error type -------------------------------------------------------

    #[derive(Debug, Error)]
    pub enum Error {
        #[error("winspool: OpenPrinter failed (does the printer exist? try --list-printers)")]
        Open,
        #[error("winspool: StartDocPrinter failed (handle may not be opened for RAW datatype, or driver rejected RAW)")]
        StartDoc,
        #[error("winspool: WritePrinter failed: wrote {written} of {total} bytes")]
        WriteShort { written: usize, total: usize },
        #[error("winspool: EndDocPrinter failed")]
        End,
        #[error("winspool: ClosePrinter failed")]
        Close,
    }

    fn to_utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn open_printer(name: &str) -> Result<Handle, Error> {
        let name_w = if name.is_empty() { Vec::new() } else { to_utf16(name) };
        let datatype_w = to_utf16("RAW");
        let defaults = PRINTER_DEFAULTSW {
            pDatatype: datatype_w.as_ptr() as *mut u16,
            pDevMode: ptr::null_mut(),
            DesiredAccess: PRINTER_ACCESS_USE,
        };
        let name_ptr = if name_w.is_empty() { ptr::null() } else { name_w.as_ptr() };
        let mut handle: Handle = ptr::null_mut();
        let ok = unsafe { OpenPrinterW(name_ptr, &mut handle, &defaults) };
        if ok == 0 {
            let _ = unsafe { GetLastError() };
            return Err(Error::Open);
        }
        Ok(handle)
    }

    pub fn close_printer(handle: Handle) -> Result<(), Error> {
        let ok = unsafe { ClosePrinter(handle) };
        if ok == 0 { Err(Error::Close) } else { Ok(()) }
    }

    pub fn print_raw_native(handle: Handle, doc_name: &str, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() { return Err(Error::StartDoc); }
        let doc_w = to_utf16(doc_name);
        let datatype_w = to_utf16("RAW");
        let di = DOC_INFO_1W {
            pDocName: doc_w.as_ptr() as *mut u16,
            pOutputFile: ptr::null_mut(),
            pDatatype: datatype_w.as_ptr() as *mut u16,
        };
        let started = unsafe { StartDocPrinterW(handle, 1, &di) };
        if started == 0 { return Err(Error::StartDoc); }
        let mut written: u32 = 0;
        let ok = unsafe { WritePrinter(handle, data.as_ptr(), data.len() as u32, &mut written) };
        if ok == 0 {
            unsafe { EndDocPrinter(handle) };
            return Err(Error::WriteShort { written: written as usize, total: data.len() });
        }
        if written as usize != data.len() {
            unsafe { EndDocPrinter(handle) };
            return Err(Error::WriteShort { written: written as usize, total: data.len() });
        }
        let ok = unsafe { EndDocPrinter(handle) };
        if ok == 0 { return Err(Error::End); }
        Ok(())
    }

    fn from_wide_ptr(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        unsafe {
            let mut len = 0usize;
            while *p.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
        }
    }

    pub fn default_printer_name() -> Option<String> {
        let mut len: u32 = 0;
        unsafe { GetDefaultPrinterW(ptr::null_mut(), &mut len) };
        if len == 0 {
            return None;
        }
        let mut buf: Vec<u16> = vec![0u16; len as usize];
        let ok = unsafe { GetDefaultPrinterW(buf.as_mut_ptr(), &mut len) };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    /// Two-call EnumPrinters pattern: first call (null buffer) reports the
    /// required buffer size via `pcbNeeded`, second call fills it. Level 5
    /// entries pack their string data after the fixed-size struct array
    /// within the same buffer, so each `pPrinterName`/`pPortName` pointer
    /// stays valid as long as `buf` is alive.
    ///
    /// `PRINTER_ATTRIBUTE_DEFAULT` is not reliably set by modern spoolers,
    /// so default-printer detection cross-references `GetDefaultPrinterW`
    /// by name instead of trusting the attribute bit.
    pub fn enum_printers() -> anyhow::Result<Vec<super::PrinterInfo>> {
        const FLAGS: u32 = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let mut needed: u32 = 0;
        let mut returned: u32 = 0;
        unsafe {
            EnumPrintersW(FLAGS, ptr::null(), 5, ptr::null_mut(), 0, &mut needed, &mut returned)
        };
        if needed == 0 {
            return Ok(Vec::new());
        }

        let mut buf: Vec<u8> = vec![0u8; needed as usize];
        let mut cb_buf = needed;
        let ok = unsafe {
            EnumPrintersW(FLAGS, ptr::null(), 5, buf.as_mut_ptr(), cb_buf, &mut needed, &mut returned)
        };
        let _ = &mut cb_buf;
        if ok == 0 {
            anyhow::bail!("EnumPrinters (level 5) failed");
        }

        let default_name = default_printer_name();
        let entry_size = std::mem::size_of::<PRINTER_INFO_5W>();
        let mut out = Vec::with_capacity(returned as usize);
        for i in 0..returned as usize {
            let entry = unsafe { &*(buf.as_ptr().add(i * entry_size) as *const PRINTER_INFO_5W) };
            let name = from_wide_ptr(entry.pPrinterName);
            let port_name = from_wide_ptr(entry.pPortName);
            let is_default = default_name.as_deref() == Some(name.as_str());
            out.push(super::PrinterInfo { name, port_name, driver_name: String::new(), is_default });
        }
        Ok(out)
    }
}

#[cfg(not(windows))]
mod imp {
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Error {
        #[error("winspool: not available on this platform")]
        Unsupported,
    }

    pub type Handle = ();

    pub fn open_printer(_name: &str) -> Result<Handle, Error> { Err(Error::Unsupported) }
    pub fn close_printer(_handle: Handle) -> Result<(), Error> { Err(Error::Unsupported) }
    pub fn print_raw_native(_handle: Handle, _doc_name: &str, _data: &[u8]) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    pub fn default_printer_name() -> Option<String> { None }

    pub fn enum_printers() -> anyhow::Result<Vec<super::PrinterInfo>> {
        Ok(vec![super::PrinterInfo {
            name: "FakePrinter (non-Windows stub)".to_string(),
            port_name: "FAKE".to_string(),
            driver_name: "Generic / Text Only".to_string(),
            is_default: true,
        }])
    }
}

pub use imp::*;

pub fn print_raw(printer_name: &str, doc_name: &str, data: &[u8]) -> anyhow::Result<()> {
    let handle = open_printer(printer_name)?;
    let res = match print_raw_native(handle, doc_name, data) {
        Ok(()) => close_printer(handle),
        Err(e) => {
            let _ = close_printer(handle);
            return Err(anyhow::anyhow!("{e}"));
        }
    };
    res.map_err(|e| anyhow::anyhow!("{e}"))
}

#[derive(Debug, Clone)]
pub struct PrinterInfo {
    pub name: String,
    pub port_name: String,
    pub driver_name: String,
    pub is_default: bool,
}

pub fn format_list(infos: &[PrinterInfo]) -> String {
    if infos.is_empty() {
        return "No printers found.\n".to_string();
    }
    let mut sb = String::new();
    sb.push_str(&format!("{:<4} {:<9} {:<55} {:<20}\n", "#", "Default", "Name", "Port"));
    sb.push_str(&"-".repeat(95));
    sb.push('\n');
    for (i, p) in infos.iter().enumerate() {
        let marker = if p.is_default { "*" } else { "" };
        let mut port = p.port_name.clone();
        if port.len() > 20 { port = format!("{}...", &port[..17]); }
        let mut name = p.name.clone();
        if name.len() > 55 { name = format!("{}...", &name[..52]); }
        sb.push_str(&format!("{:<4} {:<9} {:<55} {:<20}\n", i + 1, marker, name, port));
    }
    sb
}

pub fn filter_empty(infos: Vec<PrinterInfo>) -> Vec<PrinterInfo> {
    infos.into_iter().filter(|p| !p.name.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_raw_rejects_non_windows() {
        #[cfg(not(windows))]
        {
            let err = print_raw("", "doc", b"hello").unwrap_err().to_string();
            assert!(err.contains("not available"));
        }
    }

    #[test]
    fn filter_empty_drops_blank() {
        let v = vec![
            PrinterInfo { name: "Real".into(), port_name: "USB".into(), driver_name: "D".into(), is_default: true },
            PrinterInfo { name: "".into(), port_name: "USB".into(), driver_name: "D".into(), is_default: false },
        ];
        let out = filter_empty(v);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Real");
    }
}
