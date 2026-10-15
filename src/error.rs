use std::fmt;

/// A parse or runtime error, with the line it happened on when known.
#[derive(Debug, Clone)]
pub struct WispError {
    pub msg: String,
    pub line: u32,
    pub file: Option<String>,
}

impl WispError {
    pub fn new(msg: impl Into<String>, line: u32) -> Self {
        WispError { msg: msg.into(), line, file: None }
    }

    pub fn in_file(mut self, file: &str) -> Self {
        if self.file.is_none() {
            self.file = Some(file.to_string());
        }
        self
    }
}

impl fmt::Display for WispError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (&self.file, self.line) {
            (Some(file), l) if l > 0 => write!(f, "{}:{}: {}", file, l, self.msg),
            (Some(file), _) => write!(f, "{}: {}", file, self.msg),
            (None, l) if l > 0 => write!(f, "line {}: {}", l, self.msg),
            _ => write!(f, "{}", self.msg),
        }
    }
}

impl std::error::Error for WispError {}
