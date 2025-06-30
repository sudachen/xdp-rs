use std::net::UdpSocket;
use std::time::Duration;
use std::io;
use tokio_util::sync::CancellationToken;

pub fn run_pinger(local_addr: &str, remote_addr: &str) -> io::Result<()> {
    let socket = UdpSocket::bind(local_addr)?;
    log::debug!("[UDP_Pinger] Bound to {}", local_addr);
    socket.connect(remote_addr)?;
    log::debug!("[UDP_Pinger] Connected to {}", remote_addr);
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    let ping_message = b"PING";
    log::debug!("[UDP_Pinger] Sending 'PING' to {}...", remote_addr);
    socket.send(ping_message)?;
    let mut buffer = [0u8; 1024]; // A buffer to store received data.
    match socket.recv(&mut buffer) {
        Ok(number_of_bytes) => {
            let message = &buffer[..number_of_bytes];
            if message == b"PONG" {
                log::debug!("[UDP_Pinger] Success! Received 'PONG' from {}", remote_addr);
            } else {
                let received_str = String::from_utf8_lossy(message);
                log::error!("[UDP_Pinger] Received unexpected message: '{}'", received_str);
            }
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
            log::error!("[UDP_Pinger] Error: Did not receive a 'PONG' within 5 seconds.");
            return Err(e);
        }
        Err(e) => {
            log::error!("[UDP_Pinger] Error receiving data: {}", e);
            return Err(e);
        }
    }
    Ok(())
}

pub fn run_ponger(local_addr: &str, token: CancellationToken) -> io::Result<()> {
    let socket = UdpSocket::bind(local_addr)?;
    log::debug!("[UDP_Ponger] Listening on {}...", local_addr);
    socket.set_read_timeout(Some(Duration::from_millis(300)))?;
    let mut buffer = [0u8; 1024];
    loop {
        match socket.recv_from(&mut buffer) {
            // A packet was successfully received.
            Ok((number_of_bytes, src_addr)) => {
                let message = &buffer[..number_of_bytes];
                if message == b"PING" {
                    log::debug!("[UDP_Ponger] Received 'PING' from {}. Responding...", src_addr);
                    socket.send_to(b"PONG", src_addr)?;
                } else {
                    let received_str = String::from_utf8_lossy(message);
                    log::debug!("[UDP_Ponger] Received unexpected: '{}'. Ignoring.", received_str);
                }
                break;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                if token.is_cancelled() { break }
                continue;
            }
            Err(e) => {
                log::error!("[UDP_Ponger] A network error occurred: {}", e);
                break;
            }
        }
    }
    Ok(())
}
