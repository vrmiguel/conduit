use std::time::Duration;

use actix_web::{
    App, HttpResponse, HttpServer, get, post,
    web::{self, Bytes},
};
use arcstr::ArcStr;
use brige::{notify_sender, wait_for_receiver};
use error::Error;
use redb::Database;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::info;

/// Connects the senders to their receivers
mod brige;
/// Key-value store
mod db;
/// Error handling
mod error;

pub use db::init as init_db;
pub type Result<T> = std::result::Result<T, Error>;

/// Optional token
#[derive(Deserialize)]
struct TokenParam {
    token: Option<ArcStr>,
}

#[post("/{session_name}")]
async fn upload(
    session_name: web::Path<ArcStr>,
    param: web::Query<TokenParam>,
    db: web::Data<Database>,
    payload: web::Payload,
) -> Result<HttpResponse> {
    info!("Sender [{session_name}] connected");

    let session_name = session_name.into_inner();
    let token = param.into_inner().token;

    // Write down session
    db::create_session(session_name.clone(), token, db).await?;

    info!("Sender [{session_name}]: session created");

    // Wait for client to connect, getting its bytes sender handle once connected
    // TODO: add timeout
    let bytes_sender = wait_for_receiver(session_name.clone()).await.map_err(|_| {
        // Sender dropped before notifying
        Error::SenderTimeout
    })?;

    async fn transmit_payload(
        bytes_sender: mpsc::Sender<Result<Bytes>>,
        mut payload: web::Payload,
    ) -> Result<()> {
        while let Some(next) = payload.next().await {
            if let Err(_err) = bytes_sender.send(next.map_err(Into::into)).await {
                return Err(Error::ReceiverDisconnected);
            }
        }

        Ok(())
    }

    info!("Sender [{session_name}] to start transmitting");
    transmit_payload(bytes_sender, payload).await?;

    // TODO: ensure no WAITING_SENDER left for this session
    // TODO: cleanup in general
    debug_assert!(true);

    info!("Sender [{session_name}] finished transmitting");

    Ok(HttpResponse::Ok().finish())
}

async fn confirm_token_retry(
    session_name: ArcStr,
    token: Option<ArcStr>,
    db: web::Data<Database>,
) -> Result<()> {
    let mut retry_count = 0;
    let max_retries = 5;
    // Start with 100ms
    let mut delay_ms = 100;
    // Cap at 5 seconds
    let max_delay_ms = 5000;

    loop {
        match db::confirm_session_token(session_name.clone(), token.clone(), db.clone()).await {
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
                tracing::info!("Session does not exist, retrying");

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

#[get("/{session_name}")]
async fn download(
    session_name: web::Path<ArcStr>,
    param: web::Query<TokenParam>,
    db: web::Data<Database>,
) -> Result<HttpResponse> {
    info!("Receiver [{session_name}] connected");
    let session_name = session_name.into_inner();
    let token = param.into_inner().token;

    // Checks:
    // - if this session exists (retrying if it doesn't, to handle receivers connecting slightly ahead of the sender)
    // - if this token matches the expected token
    confirm_token_retry(session_name.clone(), token.clone(), db.clone()).await?;

    info!("Receiver [{session_name}] authenticated");

    let (sender, receiver) = mpsc::channel::<Result<Bytes>>(8);

    // If this succeeds, both sender and receiver are connected
    notify_sender(session_name.clone(), sender)?;

    info!("Receiver [{session_name}] matched with sender, starting streaming");

    let stream = tokio_stream::wrappers::ReceiverStream::new(receiver);

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .streaming(stream))
}

pub async fn run_server() -> Result<()> {
    tracing_subscriber::fmt().compact().init();

    let db = db::init("my_db.redb")?;

    let conn = web::Data::new(db);

    HttpServer::new(move || {
        App::new()
            .configure(|cfg| {
                cfg.service(upload);
                cfg.service(download);
            })
            .app_data(conn.clone())
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await?;

    Ok(())
}
