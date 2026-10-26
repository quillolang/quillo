use std::fmt;

/// A parse or runtime error, with the line it happened on when known.
#[derive(Debug, Clone)]
pub struct QuilloError {
    pub msg: String,
    pub line: u32,
    pub file: Option<String>,
}

impl QuilloError {
    pub fn new(msg: impl Into<String>, line: u32) -> Self {
        QuilloError { msg: msg.into(), line, file: None }
    }

    pub fn in_file(mut self, file: &str) -> Self {
        if self.file.is_none() {
            self.file = Some(file.to_string());
        }
        self
    }
}

impl fmt::Display for QuilloError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Shown in the terminal and, in development, on pages: values under
        // sensitive keys, card numbers and IBANs masked (see `redact`).
        let msg = crate::redact::text(&self.msg);
        match (&self.file, self.line) {
            (Some(file), l) if l > 0 => write!(f, "{}:{}: {}", file, l, msg),
            (Some(file), _) => write!(f, "{}: {}", file, msg),
            (None, l) if l > 0 => write!(f, "line {}: {}", l, msg),
            _ => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for QuilloError {}
