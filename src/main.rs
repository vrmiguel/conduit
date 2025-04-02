use conduit::{Result, run_server};

#[actix_web::main]
async fn main() -> Result<()> {
    run_server().await
}
