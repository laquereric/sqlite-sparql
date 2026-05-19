/// Error types for sqlite-sparql.
///
/// All errors are ultimately converted into SQLite error strings so they can
/// be surfaced through the normal SQLite error-reporting mechanism.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SparqlError {
    #[error("SPARQL parse error: {0}")]
    ParseError(String),

    #[error("SPARQL evaluation error: {0}")]
    EvalError(String),

    #[error("RDF parse error: {0}")]
    RdfParseError(String),

    #[error("RDF serialization error: {0}")]
    RdfSerializeError(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Store error: {0}")]
    StoreError(String),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),

    #[error("RDF-star quoted triples are not supported in sqlite-sparql 0.1.x")]
    RdfStarUnsupported,
}

impl From<SparqlError> for sqlite_loadable::Error {
    fn from(e: SparqlError) -> Self {
        sqlite_loadable::Error::new_message(&e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SparqlError>;
