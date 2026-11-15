use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub line: usize,
    pub at: String,
    pub msg: String,
    pub severity: Severity,
}

impl Diag {
    pub fn error(line: usize, at: impl Into<String>, msg: impl Into<String>) -> Diag {
        Diag {
            line,
            at: at.into(),
            msg: msg.into(),
            severity: Severity::Error,
        }
    }

    pub fn warning(line: usize, at: impl Into<String>, msg: impl Into<String>) -> Diag {
        Diag {
            severity: Severity::Warning,
            ..Diag::error(line, at, msg)
        }
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        if self.at.is_empty() {
            write!(f, "line {}: {kind}: {}", self.line, self.msg)
        } else {
            write!(f, "line {}: {kind}: {}: {}", self.line, self.at, self.msg)
        }
    }
}
