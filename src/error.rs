use actix_web::{ResponseError, http::StatusCode};
use arcstr::ArcStr;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Key-value store error: {0}")]
    Kv(#[from] redb::DatabaseError),
    #[error("Key-value transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),
    #[error("Key-value table error: {0}")]
    Table(#[from] redb::TableError),
    #[error("Storage error: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("Commit error: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("Join error: {0}")]
    Join(#[from] actix_web::rt::task::JoinError),
    #[error("Session already exists")]
    SessionExists,
    #[error("Unknown session {0}")]
    UnknownSession(ArcStr),
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
}

impl ResponseError for Error {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            Error::FailedAuthSession => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
