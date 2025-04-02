use actix_web::{
    App, HttpResponse, HttpServer, get, post,
    web::{self, Bytes},
};
use arcstr::ArcStr;
use db::{notify_sender, wait_for_receiver};
use error::Error;
use redb::Database;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

mod db;
mod error;

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
    let session_name = session_name.into_inner();
    let token = param.into_inner().token;

    // Write down session
    db::create_session(session_name.clone(), token, db).await?;

    // Wait for client to connect, getting its bytes sender handle once connected
    // TODO: add timeout
    let bytes_sender = wait_for_receiver(session_name).await.map_err(|_| {
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

    transmit_payload(bytes_sender, payload).await?;

    // TODO: ensure no WAITING_SENDER left for this session
    debug_assert!(true);

    Ok(HttpResponse::Ok().finish())
}

#[get("/{session_name}")]
async fn download(
    session_name: web::Path<ArcStr>,
    param: web::Query<TokenParam>,
    db: web::Data<Database>,
) -> Result<HttpResponse> {
    let session_name = session_name.into_inner();

    match db::confirm_session_token(session_name.clone(), param.into_inner().token, db).await {
        Ok(()) => {}
        Err(Error::UnknownSession(_)) => {
            // Session doesn't exist, but maybe the sender will connect soon
            // TODO: Implement retry logic
        }
        Err(err) => return Err(err),
    }

    let (sender, receiver) = mpsc::channel::<Result<Bytes>>(8);

    // If this succeeds, both sender and receiver are connected
    notify_sender(session_name, sender)?;

    let stream = tokio_stream::wrappers::ReceiverStream::new(receiver);

    // Return a streaming response
    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .streaming(stream))
}

#[actix_web::main]
async fn main() -> Result<()> {
    actix_web::rt::spawn(async move {
        //
    });

    let db = redb::Database::create("my_db.redb")?;

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
