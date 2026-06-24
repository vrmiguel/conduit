use std::{net::ToSocketAddrs, time::Duration};

use actix_web::{
    App, HttpResponse, HttpServer, get, put,
    web::{self, Bytes},
};
use error::Error;
use serde::Deserialize;
use session::{claim_retry, create, remove};
use small_string::SmallString;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::info;

mod error;
mod session;
mod small_string;

pub type Result<T> = std::result::Result<T, Error>;

pub type SessionName = SmallString<10, 30>;
pub type Token = SmallString<8, 30>;

/// Optional token
#[derive(Deserialize)]
struct TokenParam {
    token: Option<Token>,
}

struct PendingSession {
    session_name: SessionName,
    active: bool,
}

impl PendingSession {
    fn new(session_name: SessionName) -> Self {
        Self {
            session_name,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for PendingSession {
    fn drop(&mut self) {
        if self.active {
            remove(&self.session_name);
        }
    }
}

#[put("/{session_name}")]
async fn upload(
    session_name: web::Path<SessionName>,
    param: web::Query<TokenParam>,
    payload: web::Payload,
) -> Result<HttpResponse> {
    info!("Sender [{session_name}] connected");

    let session_name = session_name.into_inner();
    let token = param.into_inner().token;

    let receiver_wait = create(session_name.clone(), token)?;
    let mut pending_session = PendingSession::new(session_name.clone());

    info!("Sender [{session_name}]: session created");

    // Wait for client to connect, getting its bytes sender handle once connected
    let bytes_sender = match tokio::time::timeout(Duration::from_secs(5 * 60), receiver_wait).await
    {
        Ok(Ok(bytes_sender)) => bytes_sender,
        Ok(Err(_)) => return Err(Error::ReceiverDisconnected),
        Err(_) => return Err(Error::SenderTimeout),
    };
    pending_session.disarm();

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

    info!("Sender [{session_name}] finished transmitting");

    Ok(HttpResponse::Ok().finish())
}

#[get("/{session_name}")]
async fn download(
    session_name: web::Path<SessionName>,
    param: web::Query<TokenParam>,
) -> Result<HttpResponse> {
    info!("Receiver [{session_name}] connected");
    let session_name = session_name.into_inner();
    let token = param.into_inner().token;

    let (sender, receiver) = mpsc::channel::<Result<Bytes>>(128);

    // Checks:
    // - if this session exists (retrying if it doesn't, to handle receivers connecting slightly ahead of the sender)
    // - if this token matches the expected token
    claim_retry(session_name.clone(), token, sender).await?;

    info!("Receiver [{session_name}] matched with sender, starting streaming");

    let stream = tokio_stream::wrappers::ReceiverStream::new(receiver);

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .streaming(stream))
}

pub async fn run_server(addr: impl ToSocketAddrs) -> Result<()> {
    tracing_subscriber::fmt().compact().init();

    info!("Server starting in 127.0.0.1:8080");

    HttpServer::new(move || {
        App::new().configure(|cfg| {
            cfg.service(upload);
            cfg.service(download);
        })
    })
    .bind(addr)?
    .run()
    .await?;

    Ok(())
}
