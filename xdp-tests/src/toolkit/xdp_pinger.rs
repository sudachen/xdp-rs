use std::io::{Error, Result};
use std::net::Ipv4Addr;
use std::str::FromStr as _;
use std::time;
use xdp_socket::{create_tx_socket, util::{Neighbor, Router, get_ipv4_address}};

pub fn run_pinger(src_ip: &str, src_port: u16, dst_ip: &str, dst_port: u16) -> Result<()> {
    let src_addr = Ipv4Addr::from_str(src_ip)
        .map_err(|e| Error::other(format!("invalid IP address: {e}")))?;
    let dst_addr = Ipv4Addr::from_str(dst_ip)
        .map_err(|e| Error::other(format!("invalid IP address: {e}")))?;
    eprintln!("Addresses: {:#?} ", get_ipv4_address(None)
        .map_err(|e| Error::other(format!("Failed to get IP address: {e}")))?);
    let if_index = get_ipv4_address(None)?
        .iter()
        .find(|(addr, _)| *addr == src_addr)
        .ok_or_else(|| Error::other(format!("Source IP {src_ip} not found")))?
        .1;
    eprintln!("Interface index: {if_index}");
    let src_mac = eui48::MacAddress::from_str("fa:95:2c:e3:0e:a5")
        .map_err(|e| Error::other(format!("invalid MAC address: {e}")))?
        .to_array();
    let dst_mac = eui48::MacAddress::from_str("aa:79:ea:34:4b:b8")
        .map_err(|e| Error::other(format!("invalid MAC address: {e}")))?
        .to_array();

    let mut socket = create_tx_socket(if_index,0,None)
        .map_err(|e| Error::other(format!("Failed to create XDP socket: {e}")))?;

    log::debug!("Create router for interface index {if_index}");

    let mut router = Router::new(if_index);
    router
        .refresh()
        .map_err(|e| Error::other(format!("Failed to refresh router: {e}")))?;
    log::debug!("Router Neighbors: {:?}", router.neighbors);
    log::debug!("Router Routes: {:?}", router.routes);

    router.neighbors.insert(
        dst_addr,
        Neighbor {
            ip: dst_addr,
            mac: dst_mac,
            if_index,
        },
    );

    let next_hop = router
        .route(&dst_addr)
        .ok_or_else(|| Error::other(format!("No route to destination IP {dst_ip}")))?;

    log::debug!("Next hop for {dst_ip}: {next_hop:?}");
    let data = b"PING";
    let hdr = xdp_socket::util::write_udp_header_for(
        data,
        src_addr,
        src_mac,
        src_port,
        dst_addr,
        next_hop.mac_addr.unwrap(),
        dst_port,
    )?;
    loop {
        socket
            .send_blocking(data, Some(&hdr))
            .map_err(|e| Error::other(format!("Failed to write header: {e}")))?;
        log::debug!("Sent PING packet from {src_ip} to {dst_ip}");
        std::thread::sleep(time::Duration::from_millis(300));
    }
    //log::debug!("Packet completed");
    //Ok(())
}
