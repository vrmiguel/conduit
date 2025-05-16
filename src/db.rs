use std::{path::Path, time::Duration};

use actix_web::web;
use redb::{Database, ReadableTable, TableDefinition};
use tracing::info;

use crate::{Result, SessionName, Token, error::Error, token};

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
    token_opt: Option<Token>,
    db: web::Data<Database>,
) -> Result<()> {
    // Validate token if provided
    if let Some(ref token) = token_opt {
        token::validate_token(token)?;
    }

    actix_web::rt::task::spawn_blocking(move || {
        let write = db.begin_write()?;

        let mut table = write.open_table(SESSIONS)?;

        if table.get(session_name.as_str())?.is_some() {
            return Err(Error::SessionExists);
        }

        // Hash the token if it exists
        let hashed_token = if let Some(token) = token_opt {
            // Use our token hashing function
            Some(token::hash_token(token.as_str())?)
        } else {
            None
        };

        // Store the hashed token
        table.insert(
            session_name.as_str(),
            hashed_token.as_deref().map(|s| s.as_str())
        )?;

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
    // First, check if we're being rate limited
    token::check_rate_limit(session_name.as_str())?;

    actix_web::rt::task::spawn_blocking(move || {
        let read = db.begin_read()?;
        let table = read.open_table(SESSIONS)?;

        let authenticated = match table.get(&*session_name)? {
            Some(stored_hash) => {
                // If a token was required but not provided
                if stored_hash.value().is_some() && token.is_none() {
                    false
                } else if let (Some(stored), Some(provided)) = (stored_hash.value(), token.as_deref()) {
                    // Verify the provided token against the stored hash
                    match token::verify_token(provided, stored) {
                        Ok(true) => true,
                        Ok(false) => false,
                        Err(e) => {
                            tracing::error!("Token verification error: {}", e);
                            false
                        }
                    }
                } else {
                    // No token was required, and none was provided
                    stored_hash.value().is_none() && token.is_none()
                }
            },
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
            // Record the failed attempt for rate limiting
            token::record_failed_attempt(session_name.as_str());
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
