//! Stable error model shared by CLI, Host and GUI bridge.
//! Exit codes follow the v9 contract (see requirements §3.2).

use serde::Serialize;
use serde_json::{json, Value};

pub const EXIT_OK: i32 = 0;
pub const EXIT_INTERNAL: i32 = 1;
pub const EXIT_VALIDATION: i32 = 2;
pub const EXIT_NOT_FOUND: i32 = 3;
pub const EXIT_CONFLICT: i32 = 4;
pub const EXIT_IO: i32 = 5;
pub const EXIT_CONFIRM: i32 = 10;
pub const EXIT_EXEC: i32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Internal,
    Validation,
    NotFound,
    Conflict,
    Io,
    Confirmation,
    Execution,
}

impl ErrorKind {
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Internal => EXIT_INTERNAL,
            ErrorKind::Validation => EXIT_VALIDATION,
            ErrorKind::NotFound => EXIT_NOT_FOUND,
            ErrorKind::Conflict => EXIT_CONFLICT,
            ErrorKind::Io => EXIT_IO,
            ErrorKind::Confirmation => EXIT_CONFIRM,
            ErrorKind::Execution => EXIT_EXEC,
        }
    }

    pub fn type_name(self) -> &'static str {
        match self {
            ErrorKind::Internal => "internal",
            ErrorKind::Validation => "validation",
            ErrorKind::NotFound => "not_found",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Io => "io",
            ErrorKind::Confirmation => "confirmation_required",
            ErrorKind::Execution => "execution_failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoreError {
    pub kind: ErrorKind,
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
    pub details: Option<Value>,
}

impl CoreError {
    pub fn new(kind: ErrorKind, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            code: code.into(),
            message: message.into(),
            hint: None,
            details: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Validation, code, message)
    }

    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, code, message)
    }

    pub fn conflict(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, code, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Io, "IO_ERROR", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, "INTERNAL", message)
    }

    pub fn confirmation(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Confirmation, "CONFIRMATION_REQUIRED", message)
    }

    pub fn execution(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Execution, code, message)
    }

    pub fn exit_code(&self) -> i32 {
        self.kind.exit_code()
    }

    pub fn to_json(&self) -> Value {
        let mut error = json!({
            "type": self.kind.type_name(),
            "code": self.code,
            "message": self.message,
        });
        if let Some(hint) = &self.hint {
            error["hint"] = json!(hint);
        }
        if let Some(details) = &self.details {
            error["details"] = details.clone();
        }
        error
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CoreError {}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        CoreError::io(error.to_string())
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(error: serde_json::Error) -> Self {
        CoreError::new(ErrorKind::Io, "JSON_ERROR", error.to_string())
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
