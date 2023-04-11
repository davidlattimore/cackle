use rustc_demangle::demangle;
use std::fmt::Display;

#[derive(Default, Debug, Clone)]
pub(crate) struct SectionName {
    bytes: Vec<u8>,
}

/// The name of a linker section. e.g. ".text.foo". Allows invalid UTF-8, but when it is valid
/// UTF-8, displays nicely, including demangling.
impl SectionName {
    pub(crate) fn new<T: Into<Vec<u8>>>(bytes: T) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }
}

impl Display for SectionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Ok(sym_string) = std::str::from_utf8(&self.bytes) {
            if let Some(i) = sym_string.find("._") {
                write!(
                    f,
                    "{}{}",
                    &sym_string[..i + 1],
                    demangle(&sym_string[i + 1..])
                )?;
            } else {
                write!(f, "{}", sym_string)?;
            }
        } else {
            write!(f, "INVALID-UTF-8({:?})", &self.bytes)?;
        }
        Ok(())
    }
}
