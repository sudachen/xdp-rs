use std::env;
use std::error::Error;
use tokio::net::UdpSocket;
use std::str;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <ip_address:port>", args[0]);
        return Err("Invalid number of arguments".into());
    }

    let addr = &args[1];
    let socket = UdpSocket::bind(addr).await?;
    log::info!("Listening on: {}", socket.local_addr()?);

    let mut buf = [0; 1024];

    loop {
        let (len, peer) = socket.recv_from(&mut buf).await?;
        let message = match str::from_utf8(&buf[..len]) {
            Ok(s) => s.trim(),
            Err(_) => {
                log::warn!("Received non-UTF8 data from {}", peer);
                continue;
            }
        };

        log::debug!("Received {} bytes from {}: {}", len, peer, message);

        if message == "PING" {
            log::info!("Received PING from {}, sending PONG", peer);
            socket.send_to(b"PONG", peer).await?;
        }
    }
}