//
// pingpong.rs - Integration Test for XDP-based Ping
//
// Purpose:
//   This file contains an integration test to verify the end-to-end functionality of the XDP
//   pinger. It ensures that the XDP program can correctly assemble, address, and transmit a
//   UDP packet that can be received and understood by a standard UDP server.
//
// How it works:
//   - It requires elevated privileges (CAP_NET_ADMIN, CAP_NET_RAW) to create network namespaces
//     and use AF_XDP sockets.
//   - It uses a test harness (`suite::runner`) to create a virtual network environment with a
//     pair of connected hosts (`host0` and `host1`).
//   - A standard UDP "pong" server is started on `host1`.
//   - The XDP-based pinger (`xdp::xdp_pinger`) is started on `host0`, which sends a ping to the
//     pong server on `host1`.
//   - The test succeeds if the ping-pong communication completes successfully.
//

pub mod suite;
pub mod xdp;

use std::io::Result;

#[tokio::main]
pub async fn main() -> Result<()> {
    suite::command::setup(&[
        caps::Capability::CAP_NET_ADMIN,
        caps::Capability::CAP_NET_RAW,
    ])?;
    let e = suite::runner::run_test_with_pair(|host_pair| async move {
        log::debug!("Running test");
        log::info!("starting pong host on {}", host_pair.host1.if_dev);
        let host1_ip = host_pair.host1.ip.clone();
        let ponger_shutdown = tokio_util::sync::CancellationToken::new();
        let token = ponger_shutdown.clone();
        let ponger = tokio::task::spawn_blocking(move || {
            match suite::udp_pingpong::run_ponger(&format!("{host1_ip}:9001"), token) {
                Ok(_) => log::info!("Ponger completed successfully on {}", host1_ip),
                Err(e) => log::error!("Failed to complete ponger on {}: {}", host1_ip, e),
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await; // Give ponger time to start
        log::info!("starting ping host on {}", host_pair.host0.if_dev);
        let host0_ip = host_pair.host0.ip.clone();
        let host1_ip = host_pair.host1.ip.clone();
        let pinger = tokio::task::spawn_blocking(move || {
            match xdp::xdp_pinger(&host0_ip, 9000, &host1_ip, 9001) {
                Ok(_) => log::info!("Pinger completed successfully on {}", host0_ip),
                Err(e) => log::error!("Failed to complete pinger on {}: {}", host0_ip, e),
            }
            // match suite::udp_pingpong::run_pinger(
            //     &format!("{host0_ip}:9001"),
            //     &format!("{host1_ip}:9000")) {
            //     Ok(_) => log::info!("Pinger completed successfully on {}", host0_ip),
            //     Err(e) => log::error!("Failed to complete pinger on {}: {}", host0_ip, e),
            // }
        });
        pinger.await?;
        //ponger_shutdown.cancel();
        ponger.await?;
        Ok(())
    })
    .await;
    if let Err(e) = e {
        log::error!("Pingpong test failed: {e}");
        Err(e)
    } else {
        log::info!("Pingpong test passed.");
        Ok(())
    }
}
