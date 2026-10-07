use clap::Parser;
use rm_server_sync::{Service, http};
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:7878")]
    address: SocketAddr,
    #[arg(long, default_value_t = 300)]
    token_ttl_seconds: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.token_ttl_seconds == 0 {
        return Err("--token-ttl-seconds must be positive".into());
    }
    http::run(
        args.address,
        Service::with_token_ttl(Duration::from_secs(args.token_ttl_seconds)),
    )
}
