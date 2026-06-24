use std::{sync::LazyLock, time::Duration};

use actix_web::web::Bytes;
use dashmap::{DashMap, mapref::entry::Entry};
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use crate::{Result, SessionName, Token, error::Error};

/// For the uploader to transmit bytes to the downloader
pub type ByteSender = mpsc::Sender<Result<Bytes>>;

struct Session {
    token: Option<Token>,
    sender: oneshot::Sender<ByteSender>,
}

static SESSIONS: LazyLock<DashMap<SessionName, Session>> = LazyLock::new(DashMap::new);

/// Registers an uploader as waiting for a receiver
pub fn create(
    session_name: SessionName,
    token: Option<Token>,
) -> Result<oneshot::Receiver<ByteSender>> {
    let (sender, receiver) = oneshot::channel();

    match SESSIONS.entry(session_name) {
        Entry::Occupied(_) => Err(Error::SessionExists),
        Entry::Vacant(entry) => {
            entry.insert(Session { token, sender });
            Ok(receiver)
        }
    }
}

/// Removes a session if it is still waiting to be claimed
pub fn remove(session_name: &SessionName) {
    SESSIONS.remove(session_name);
}

/// Claims a waiting session, retrying briefly for receivers that arrive first.
pub async fn claim_retry(
    session_name: SessionName,
    token: Option<Token>,
    bytes_sender: ByteSender,
) -> Result<()> {
    let mut retry_count = 0;
    let max_retries = 5;
    let mut delay_ms = 100;
    let max_delay_ms = 5000;

    loop {
        match claim(session_name.clone(), token.clone(), bytes_sender.clone()) {
            Ok(()) => return Ok(()),
            Err(Error::UnknownSession(session)) => {
                retry_count += 1;
                if retry_count > max_retries {
                    info!(
                        "Giving up after {} retries for session [{session_name}]",
                        max_retries
                    );
                    return Err(Error::UnknownSession(session));
                }

                info!(
                    "Session [{session_name}] not found, retrying in {}ms (attempt {}/{})",
                    delay_ms, retry_count, max_retries
                );

                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms = std::cmp::min(delay_ms * 2, max_delay_ms);
            }
            Err(err) => return Err(err),
        }
    }
}

fn claim(session_name: SessionName, token: Option<Token>, bytes_sender: ByteSender) -> Result<()> {
    let Some((_session_name, session)) = SESSIONS
        .remove_if(&session_name, |_session_name, session| {
            token.as_deref() == session.token.as_deref()
        })
    else {
        return if SESSIONS.contains_key(&session_name) {
            Err(Error::FailedAuthSession)
        } else {
            Err(Error::UnknownSession(session_name))
        };
    };

    session.sender.send(bytes_sender).map_err(|_| {
        Error::BadConduit(format!(
            "Sender for {session_name} dropped before receiver could claim it"
        ))
    })
}
