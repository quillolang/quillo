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

/// After a syntax error: which quillo this is, as code written for a newer
/// one fails to parse here (an old build left on the PATH, say).
pub fn which_quillo() -> String {
    let exe = std::env::current_exe().ok();
    let built = exe
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok()?.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            // Days since 1970 to a date (Howard Hinnant's civil_from_days).
            let z = (d.as_secs() / 86_400) as i64 + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let day = doy - (153 * mp + 2) / 5 + 1;
            let month = if mp < 10 { mp + 3 } else { mp - 9 };
            let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
            format!(", built {:04}-{:02}-{:02}", year, month, day)
        })
        .unwrap_or_default();
    let at = exe.map(|p| format!(" at {}", p.display())).unwrap_or_default();
    format!("this is quillo {}{}{}; code written for a newer Quillo fails to parse like this", crate::version(), built, at)
}
