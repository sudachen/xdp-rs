use std::io::{Error, Result};
use std::net::Ipv4Addr;
use std::str::FromStr;
use xdp_socket::{AfXdpSocket, DeviceQueue, Direction, Router, get_ipv4_address, Neighbor};

pub fn xdp_pinger(src_ip: &str, dst_ip: &str, _port: u16) -> Result<()> {
    let src_addr = Ipv4Addr::from_str(src_ip)
        .map_err(|e| Error::other(format!("invalid IP address: {}", e)))?;
    let dst_addr = Ipv4Addr::from_str(dst_ip)
        .map_err(|e| Error::other(format!("invalid IP address: {}", e)))?;
    let if_index = get_ipv4_address(None)?
        .iter()
        .find(|(addr, _)| *addr == src_addr)
        .ok_or_else(|| Error::other(format!("Source IP {} not found", src_ip)))?
        .1;

    let mut socket = AfXdpSocket::new(DeviceQueue::form_ifindex(3), Direction::Tx, None)
        .map_err(|e| Error::other(format!("Failed to create XDP socket: {}", e)))?;

    log::debug!("Create router for interface index {}", if_index);
    
    let mut router = Router::new(if_index);
    router
        .refresh()
        .map_err(|e| Error::other(format!("Failed to refresh router: {}", e)))?;
    log::debug!("Router Neighbors: {:?}", router.neighbors);
    log::debug!("Router Routes: {:?}", router.routes);

    router.neighbors.insert(dst_addr, Neighbor {
        ip: dst_addr,
        mac: [0u8;6],
        if_index
    });
    
    let next_hop = router
        .route(dst_addr)
        .ok_or_else(|| Error::other(format!("No route to destination IP {}", dst_ip)))?;

    log::debug!("Next hop for {}: {:?}", dst_ip, next_hop);
    socket
        .tx()?
        .send_and_wakeup(
            &[
                0x45, 0x00, 0x00, 0x3c, 0x1c, 0x46, 0x40, 0x00, 0x40, 0x06, 0xb1, 0xe6,
            ], // IP header
            None,
        )
        .map_err(|e| Error::other(format!("Failed to write header: {:?}", e)))?;
    Ok(())
}
