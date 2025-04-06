use std::sync::LazyLock;

use crate::{Result, SessionName, error::Error};
use actix_web::web::Bytes;
use dashmap::DashMap;
use tokio::sync::{mpsc, oneshot};

/// Used for the uploader to transmit bytes to the downloader
pub type ByteSender = mpsc::Sender<Result<Bytes>>;

static WAITING_SENDERS: LazyLock<DashMap<SessionName, oneshot::Sender<ByteSender>>> =
    LazyLock::new(DashMap::new);

/// Registers this sender as waiting for a receiver
///
/// Returns a handle to notify the sender when the receiver is available
pub fn wait_for_receiver(session_name: SessionName) -> oneshot::Receiver<ByteSender> {
    let (sender, receiver) = oneshot::channel();
    WAITING_SENDERS.insert(session_name, sender);

    receiver
}

/// A receiver notifies a sender that it is available, and sends
/// the sender a handle for file transfer.
pub fn notify_sender(session_name: SessionName, bytes_sender: ByteSender) -> Result<()> {
    let sender = WAITING_SENDERS
        .remove(&session_name)
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
