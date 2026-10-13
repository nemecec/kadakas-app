//! Errors of the feedback code.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackError {
    /// The input breaks a rule (a missing title, a wrong image, a size).
    Invalid(String),
    /// A limit stops the call: the outbox is full or the session sent too many reports.
    Limit(String),
    /// No report has this id.
    NotFound(String),
    /// The report is in a status that does not allow the call.
    State(String),
    /// The store failed.
    Store(String),
}

impl std::fmt::Display for FeedbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedbackError::Invalid(m)
            | FeedbackError::Limit(m)
            | FeedbackError::NotFound(m)
            | FeedbackError::State(m)
            | FeedbackError::Store(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for FeedbackError {}
