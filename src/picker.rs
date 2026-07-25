//! Interactive printer picker.

use std::io::{BufRead, Write};

use crate::winspool::{self, PrinterInfo};

#[derive(Debug, thiserror::Error)]
pub enum PickError {
    #[error("winspool: printer selection cancelled")]
    Cancelled,
    #[error("enumerate printers: {0}")]
    Enumerate(#[from] anyhow::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub trait Picker {
    fn pick(&self) -> Result<String, PickError>;
}

pub struct StaticPicker {
    pub name: String,
}

impl Picker for StaticPicker {
    fn pick(&self) -> Result<String, PickError> {
        Ok(self.name.clone())
    }
}

pub struct InteractivePicker<'a> {
    pub input: Box<dyn BufRead + 'a>,
    pub output: Box<dyn Write + 'a>,
    pub fallback: String,
}

impl<'a> InteractivePicker<'a> {
    pub fn pick(&mut self) -> Result<String, PickError> {
        let infos = winspool::enum_printers()?;
        let infos = winspool::filter_empty(infos);
        self.pick_from_infos(&infos)
    }

    pub fn pick_from_infos(&mut self, infos: &[PrinterInfo]) -> Result<String, PickError> {
        if infos.is_empty() {
            return Ok(self.fallback.clone());
        }
        let out: &mut dyn Write = self.output.as_mut();
        writeln!(out, "")?;
        writeln!(out, "Available printers:")?;
        write!(out, "{}", winspool::format_list(infos))?;
        writeln!(out, "")?;
        writeln!(out, "No --printer flag given. Enter the number to select,")?;
        writeln!(out, "or press Enter to use the default (marked with *).")?;
        write!(out, "> ")?;
        out.flush()?;

        let mut line = String::new();
        let n = self.input.as_mut().read_line(&mut line)?;
        if n == 0 {
            if !self.fallback.is_empty() {
                return Ok(self.fallback.clone());
            }
            return Err(PickError::Cancelled);
        }
        let line = line.trim();
        if line.is_empty() {
            for inf in infos {
                if inf.is_default {
                    return Ok(inf.name.clone());
                }
            }
            return Ok(self.fallback.clone());
        }
        if let Ok(idx) = line.parse::<usize>() {
            if idx >= 1 && idx <= infos.len() {
                return Ok(infos[idx - 1].name.clone());
            }
        }
        for inf in infos {
            if inf.name.eq_ignore_ascii_case(line) {
                return Ok(inf.name.clone());
            }
        }
        Ok(line.to_string())
    }
}

pub fn default_with_fallback(name: &str, fallback: &str) -> String {
    if !name.is_empty() { name.to_string() } else { fallback.to_string() }
}

pub fn is_interactive() -> bool {
    std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn infos() -> Vec<PrinterInfo> {
        vec![
            PrinterInfo { name: "Epson TM-T20III".into(), port_name: "USB001".into(), driver_name: "Epson".into(), is_default: true },
            PrinterInfo { name: "Microsoft Print to PDF".into(), port_name: "PORTPROMPT:".into(), driver_name: "Microsoft".into(), is_default: false },
        ]
    }

    fn picker(input: &str) -> InteractivePicker<'_> {
        InteractivePicker {
            input: Box::new(Cursor::new(input.as_bytes().to_vec())),
            output: Box::new(Vec::new()),
            fallback: String::new(),
        }
    }

    #[test]
    fn pick_by_number() {
        assert_eq!(picker("2\n").pick_from_infos(&infos()).unwrap(), "Microsoft Print to PDF");
    }

    #[test]
    fn pick_empty_uses_default() {
        assert_eq!(picker("\n").pick_from_infos(&infos()).unwrap(), "Epson TM-T20III");
    }

    #[test]
    fn pick_by_name_case_insensitive() {
        assert_eq!(picker("epson tm-t20iii\n").pick_from_infos(&infos()).unwrap(), "Epson TM-T20III");
    }

    #[test]
    fn pick_invalid_number_freeform() {
        assert_eq!(picker("99\n").pick_from_infos(&infos()).unwrap(), "99");
    }

    #[test]
    fn pick_empty_list_returns_fallback() {
        assert_eq!(picker("").pick_from_infos(&[]).unwrap(), "");
    }

    #[test]
    fn default_with_fallback_helper() {
        assert_eq!(default_with_fallback("real", "fb"), "real");
        assert_eq!(default_with_fallback("", "fb"), "fb");
    }
}
