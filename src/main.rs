use std::net::Ipv4Addr;

use conduit::{Result, run_server};

#[actix_web::main]
async fn main() -> Result<()> {
    let addr = Ipv4Addr::new(0, 0, 0, 0);

    run_server((addr, 8080)).await
}
