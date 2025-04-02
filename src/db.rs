use std::{path::Path, sync::LazyLock};

use actix_web::web::{self, Bytes};
use arcstr::ArcStr;
use dashmap::DashMap;
use redb::{Database, ReadableTable, TableDefinition};

use crate::{Result, error::Error};

use tokio::sync::{mpsc, oneshot};

/// Used for the uploader to transmit bytes to the downloader
pub type ByteSender = mpsc::Sender<Result<Bytes>>;

const SESSIONS: TableDefinition<&str, Option<&str>> = TableDefinition::new("sessions");
static WAITING_SENDERS: LazyLock<DashMap<ArcStr, oneshot::Sender<ByteSender>>> =
    LazyLock::new(DashMap::new);

pub fn init(path: impl AsRef<Path>) -> Result<Database> {
    let db = redb::Database::create(path)?;

    let write = db.begin_write()?;

    // Some write has to occur to every defined redb table
    write.open_table(SESSIONS)?;

    write.commit()?;

    Ok(db)
}

/// Registers this sender as waiting for a receiver
///
/// Returns a handle to notify the sender when the receiver is available
pub fn wait_for_receiver(session_name: ArcStr) -> oneshot::Receiver<ByteSender> {
    let (sender, receiver) = oneshot::channel();
    WAITING_SENDERS.insert(session_name, sender);

    receiver
}

/// Registers this sender as waiting for a receiver
///
/// Returns a handle to notify the sender when the receiver is available
pub fn notify_sender(session_name: ArcStr, bytes_sender: ByteSender) -> Result<()> {
    let sender = WAITING_SENDERS
        .remove(&*session_name)
        .map(|(_session_name, sender)| sender)
        .ok_or_else(|| Error::BadConduit(format!(
            "No WAITING_SENDER for {session_name} even though confirm_session_token was Ok(true)",
        )))?;

    sender.send(bytes_sender).map_err(|_| {
        // Relies that a timed-out sender has been removed from WAITING_SENDERS
        Error::BadConduit(format!(
            "Receiver for {session_name} has dropped by the time of `send`"
        ))
    })
}

pub async fn create_session(
    session_name: ArcStr,
    token: Option<ArcStr>,
    db: web::Data<Database>,
) -> Result<()> {
    actix_web::rt::task::spawn_blocking(move || {
        let write = db.begin_write()?;

        let mut table = write.open_table(SESSIONS)?;

        if table.get(&*session_name)?.is_some() {
            return Err(Error::SessionExists);
        }

        table.insert(&*session_name, token.as_deref())?;

        drop(table);
        write.commit()?;

        Ok(())
    })
    .await?
}

/// Checks if the given token matches the session token
pub async fn confirm_session_token(
    session_name: ArcStr,
    token: Option<ArcStr>,
    db: web::Data<Database>,
) -> Result<()> {
    actix_web::rt::task::spawn_blocking(move || {
        let read = db.begin_read()?;

        let table = read.open_table(SESSIONS)?;

        let authenticated = match table.get(&*session_name)? {
            Some(expected_token) => token.as_deref() == expected_token.value(),
            None => return Err(Error::UnknownSession(session_name)),
        };

        drop(table);
        drop(read);

        if authenticated {
            let write = db.begin_write()?;
            let mut table = write.open_table(SESSIONS)?;

            table.remove(&*session_name)?;
            drop(table);
            write.commit()?;

            Ok(())
        } else {
            Err(Error::FailedAuthSession)
        }
    })
    .await?
}
