use std::io::{Error, Result};
use std::net::Ipv4Addr;
use std::str::FromStr;
use xdp_socket::{AfXdpSocket, DeviceQueue, Direction, Router, get_ipv4_address, Neighbor};

pub fn xdp_pinger(src_ip: &str, src_port: u16, dst_ip: &str, dst_port: u16) -> Result<()> {
    let src_addr = Ipv4Addr::from_str(src_ip)
        .map_err(|e| Error::other(format!("invalid IP address: {}", e)))?;
    let dst_addr = Ipv4Addr::from_str(dst_ip)
        .map_err(|e| Error::other(format!("invalid IP address: {}", e)))?;
    let if_index = get_ipv4_address(None)?
        .iter()
        .find(|(addr, _)| *addr == src_addr)
        .ok_or_else(|| Error::other(format!("Source IP {} not found", src_ip)))?
        .1;

    let src_mac = eui48::MacAddress::from_str("aa:79:ea:34:4b:b8").map_err(|e| Error::other(format!("invalid MAC address: {}", e)))?.to_array();
    let dst_mac = eui48::MacAddress::from_str("fa:95:2c:e3:0e:a5").map_err(|e| Error::other(format!("invalid MAC address: {}", e)))?.to_array();

    let mut socket = AfXdpSocket::new(DeviceQueue::form_ifindex(if_index), Direction::Tx, None)
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
        mac: dst_mac,
        if_index
    });

    let next_hop = router
        .route(dst_addr)
        .ok_or_else(|| Error::other(format!("No route to destination IP {}", dst_ip)))?;

    log::debug!("Next hop for {}: {:?}", dst_ip, next_hop);
    let data = b"PING";
    let hdr = xdp_socket::packet::write_udp_header_for(
        data,
        src_addr,
        src_mac,
        src_port,
        dst_addr,
        next_hop.mac_addr.unwrap(),
        dst_port
    )?;
    socket
        .tx()?
        .send_and_wakeup(data,Some(&hdr))
        .map_err(|e| Error::other(format!("Failed to write header: {:?}", e)))?;
    log::debug!("Sent PING packet from {} to {}", src_ip, dst_ip);
    socket
        .tx()?
        .wait_for_completion()
        .map_err(|e| Error::other(format!("Failed to wait for completion: {:?}", e)))?;
    log::debug!("Packet completed");
    Ok(())
}
