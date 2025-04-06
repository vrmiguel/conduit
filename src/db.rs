use std::{path::Path, time::Duration};

use actix_web::web;
use redb::{Database, ReadableTable, TableDefinition};
use tracing::info;

use crate::{Result, SessionName, Token, error::Error};

const SESSIONS: TableDefinition<&str, Option<&str>> = TableDefinition::new("sessions");

pub fn init(path: impl AsRef<Path>) -> Result<Database> {
    let db = redb::Database::create(path)?;

    let write = db.begin_write()?;

    // Some write has to occur to every defined redb table
    write.open_table(SESSIONS)?;

    write.commit()?;

    Ok(db)
}

pub async fn create_session(
    session_name: SessionName,
    token: Option<Token>,
    db: web::Data<Database>,
) -> Result<()> {
    actix_web::rt::task::spawn_blocking(move || {
        let write = db.begin_write()?;

        let mut table = write.open_table(SESSIONS)?;

        if table.get(session_name.as_str())?.is_some() {
            return Err(Error::SessionExists);
        }

        table.insert(session_name.as_str(), token.as_deref())?;

        drop(table);
        write.commit()?;

        Ok(())
    })
    .await?
}

/// Checks if the given token matches the session token
pub async fn confirm_session_token(
    session_name: SessionName,
    token: Option<Token>,
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

/// Confirm a session token, retrying if the session doesn't exist yet
pub async fn confirm_token_retry(
    session_name: SessionName,
    token: Option<Token>,
    db: web::Data<Database>,
) -> Result<()> {
    let mut retry_count = 0;
    let max_retries = 5;
    // Start with 100ms
    let mut delay_ms = 100;
    // Cap at 5 seconds
    let max_delay_ms = 5000;

    loop {
        match confirm_session_token(session_name.clone(), token.clone(), db.clone()).await {
            Ok(()) => {
                // Authentication successful
                return Ok(());
            }
            Err(Error::UnknownSession(session)) => {
                // Session doesn't exist, retry with backoff
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
            Err(err) => {
                tracing::error!("Failed to confirm session token: {err}");
                return Err(err);
            }
        }
    }
}
