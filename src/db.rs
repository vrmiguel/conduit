use std::path::Path;

use actix_web::web;
use arcstr::ArcStr;
use redb::{Database, ReadableTable, TableDefinition};

use crate::{Result, error::Error};

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
