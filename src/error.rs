use actix_web::{ResponseError, http::StatusCode};

use crate::SessionName;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Key-value store error: {0}")]
    Kv(String),
    #[error("Join error: {0}")]
    Join(#[from] actix_web::rt::task::JoinError),
    #[error("Session already exists")]
    SessionExists,
    #[error("Unknown session {0}")]
    UnknownSession(SessionName),
    #[error("Failed session authentication")]
    FailedAuthSession,
    #[error("Timeout waiting for client connection")]
    SenderTimeout,
    #[error("Receiver disconnected")]
    ReceiverDisconnected,
    #[error("Some logic problem in Conduit's code: {0}")]
    BadConduit(String),
    #[error("Payload error: {0}")]
    Payload(#[from] actix_web::error::PayloadError),
    #[error("Session names must have a minimum length of 10")]
    MinimumSessionLength,
    #[error("Tokens must have a minimum length of 8")]
    TokenLength,
}

impl From<redb::StorageError> for Error {
    fn from(err: redb::StorageError) -> Self {
        Error::Kv(err.to_string())
    }
}

impl From<redb::DatabaseError> for Error {
    fn from(err: redb::DatabaseError) -> Self {
        Error::Kv(err.to_string())
    }
}

impl From<redb::TableError> for Error {
    fn from(err: redb::TableError) -> Self {
        Error::Kv(err.to_string())
    }
}

impl From<redb::CommitError> for Error {
    fn from(err: redb::CommitError) -> Self {
        Error::Kv(err.to_string())
    }
}

impl From<redb::TransactionError> for Error {
    fn from(err: redb::TransactionError) -> Self {
        Error::Kv(err.to_string())
    }
}

impl ResponseError for Error {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            Error::FailedAuthSession => StatusCode::UNAUTHORIZED,
            Error::UnknownSession(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
