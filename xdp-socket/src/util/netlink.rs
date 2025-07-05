//! # Low-Level Netlink Interface
//!
//! ## Purpose
//!
//! This module provides functions to query the Linux kernel's networking subsystems via
//! netlink sockets. It is used to fetch information such as network interface details,
//! IP addresses, routes, and neighbor (ARP) entries.
//!
//! ## How it works
//!
//! It communicates with the kernel using a raw `NETLINK_ROUTE` socket. The `netlink-packet`
//! crates are used to construct, serialize, and deserialize netlink messages. A generic
//! `netlink` function handles the common pattern of sending a request and processing a
//! potentially multi-part response. Specific functions like `get_ipv4_routes` and
//! `get_neighbors` use this generic handler to fetch and parse different types of data.
//!
//! ## Main components
//!
//! - `netlink()`: A generic function for the netlink request/response message loop.
//! - `get_links()`, `get_ipv4_routes()`, `get_neighbors()`, `get_ipv4_address()`: Public
//!   functions for querying specific kernel data.
//! - `Link`, `Ipv4Route`, `Neighbor`: Structs that represent the networking objects
//!   retrieved from the kernel.

use netlink_packet_core::{
    NLM_F_DUMP, NLM_F_REQUEST, NetlinkDeserializable, NetlinkMessage, NetlinkPayload,
    NetlinkSerializable,
};
use netlink_packet_route::{
    AddressFamily, RouteNetlinkMessage,
    address::{AddressAttribute, AddressMessage},
    link::{LinkAttribute, LinkMessage},
    neighbour::{NeighbourAddress, NeighbourAttribute, NeighbourMessage},
    route::{RouteAddress, RouteAttribute, RouteMessage},
};
use netlink_sys::{Socket, SocketAddr};
use std::io;
use std::net::{IpAddr, Ipv4Addr};

/// Represents a network interface link.
#[derive(Clone, Debug, Default)]
pub struct Link {
    /// The interface index.
    pub if_index: u32,
    /// The interface name (e.g., "eth0").
    pub name: String,
    /// The Maximum Transmission Unit (MTU) of the interface.
    pub mtu: u32,
    /// The MAC address of the interface.
    pub mac: [u8; 6],
}

/// Represents an IPv4 route.
#[derive(Clone, Copy, Debug)]
pub struct Ipv4Route {
    /// The destination prefix length (CIDR).
    pub dest_prefix: u8,
    /// The destination IPv4 address.
    pub destination: Ipv4Addr,
    /// The gateway IP address, if any.
    pub gateway: Option<Ipv4Addr>,
    /// The index of the output interface.
    pub out_if_index: Option<u32>,
    /// The priority of the route. Lower values are preferred.
    pub priority: u32,
}

/// Represents a neighbor (ARP) entry.
#[derive(Clone, Copy, Debug)]
pub struct Neighbor {
    /// The neighbor's IPv4 address.
    pub ip: Ipv4Addr,
    /// The neighbor's MAC address.
    pub mac: [u8; 6],
    /// The index of the interface this neighbor is associated with.
    pub if_index: u32,
}

/// Represents a default gateway.
#[derive(Clone, Copy, Debug)]
pub struct Gateway {
    /// The gateway's IPv4 address.
    pub ip: Ipv4Addr,
    /// The priority of the route to this gateway.
    pub priority: u32,
    /// The index of the output interface for this gateway.
    pub if_index: u32,
}

/// A generic function to send a netlink request and parse the response.
///
/// This function handles the low-level details of creating a netlink socket,
/// sending a request message, and iterating through the potentially multi-part
/// response from the kernel.
///
/// # How it works
///
/// It opens a `NETLINK_ROUTE` socket and binds it. The provided request message
/// is serialized and sent to the kernel. It then enters a loop, receiving
/// response messages from the socket. Each message is deserialized and passed to
/// the provided closure `f` for processing. The loop continues until all parts
/// of the kernel's response have been read. The results from the closure are
/// collected into a `Vec` and returned.
pub fn netlink<T, F, R>(mut req: NetlinkMessage<T>, f: F) -> Result<Vec<R>, io::Error>
where
    T: NetlinkSerializable + NetlinkDeserializable,
    F: Fn(NetlinkMessage<T>) -> Result<Option<R>, io::Error>,
{
    let mut socket = Socket::new(netlink_sys::constants::NETLINK_ROUTE)?;
    let kernel_addr = SocketAddr::new(0, 0);
    socket.bind(&kernel_addr)?;
    req.header.flags = NLM_F_REQUEST | NLM_F_DUMP;
    let mut send_buf = vec![0u8; req.buffer_len()];
    req.finalize();
    req.serialize(&mut send_buf);
    if socket.send(send_buf.as_slice(), 0)? != send_buf.len() {
        return Err(io::Error::other("Failed to send request"));
    };

    let (recv_buf, _) = socket.recv_from_full()?;
    let mut buffer_view = &recv_buf[..];
    let mut result = Vec::new();
    while !buffer_view.is_empty() {
        let msg = NetlinkMessage::<T>::deserialize(buffer_view).map_err(io::Error::other)?;
        let len = msg.header.length as usize;
        if let Some(r) = f(msg)? {
            result.push(r);
        }
        if len == 0 || len > buffer_view.len() {
            return Err(io::Error::other(
                "Received a malformed netlink message (invalid length)".to_string(),
            ));
        }
        buffer_view = &buffer_view[len..];
    }
    Ok(result)
}

/// Retrieves a list of neighbor (ARP) entries from the kernel.
///
/// Optionally filters neighbors by a specific interface index.
///
/// # How it works
///
/// It constructs a `GetNeighbour` netlink request and sends it using the generic
/// `netlink` function. The response parsing closure extracts neighbor details
/// like IP address, MAC address, and interface index from each `NewNeighbour`
/// message. If an `if_index` is provided, it filters the results to include
/// only neighbors on that interface.
pub fn get_neighbors(if_index: Option<u32>) -> Result<Vec<Neighbor>, io::Error> {
    let mut req_msg = NeighbourMessage::default();
    req_msg.header.family = AddressFamily::Inet;
    let req = NetlinkMessage::from(RouteNetlinkMessage::GetNeighbour(req_msg));
    netlink(req, |msg| match msg.payload {
        NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewNeighbour(neigh_msg)) => {
            if if_index.is_some_and(|x| x != neigh_msg.header.ifindex) {
                return Ok(None); // Skip neighbors not matching the interface index
            }
            let mut neighbor = Neighbor {
                ip: Ipv4Addr::UNSPECIFIED,
                mac: [0; 6],
                if_index: neigh_msg.header.ifindex,
            };
            for attr in neigh_msg.attributes.iter() {
                match attr {
                    NeighbourAttribute::Destination(NeighbourAddress::Inet(ip)) => {
                        neighbor.ip = *ip;
                    }
                    NeighbourAttribute::LinkLocalAddress(mac) => {
                        if mac.len() == 6 {
                            neighbor.mac = mac[0..6].try_into().unwrap();
                        } else {
                            return Ok(None);
                        }
                    }
                    //NeighbourAttribute::CacheInfo(_nfo) => {
                    //
                    //}
                    _ => {}
                }
            }
            Ok(Some(neighbor))
        }
        _ => Ok(None),
    })
}

/// Retrieves a list of IPv4 routes from the kernel.
///
/// Optionally filters routes by a specific output interface index.
///
/// # How it works
///
/// It constructs a `GetRoute` netlink request for the IPv4 address family and
/// sends it using the generic `netlink` function. The response parsing closure
/// processes each `NewRoute` message, extracting attributes like destination,
/// gateway, and priority. If an `if_index` is provided, it filters the results
/// to include only routes that use that output interface.
pub fn get_ipv4_routes(if_index: Option<u32>) -> Result<Vec<Ipv4Route>, io::Error> {
    let mut req_msg = RouteMessage::default();
    req_msg.header.address_family = AddressFamily::Inet;
    //req_msg.header.destination_prefix_length = 32;
    let req = NetlinkMessage::from(RouteNetlinkMessage::GetRoute(req_msg));
    netlink(req, |msg| {
        match msg.payload {
            NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewRoute(ref route_msg)) => {
                let mut route = Ipv4Route {
                    dest_prefix: route_msg.header.destination_prefix_length,
                    destination: Ipv4Addr::UNSPECIFIED,
                    gateway: None,
                    out_if_index: None,
                    priority: 0,
                };
                for a in route_msg.attributes.iter() {
                    match a {
                        RouteAttribute::Destination(RouteAddress::Inet(dest)) => {
                            route.destination = *dest;
                        }
                        RouteAttribute::Gateway(RouteAddress::Inet(gateway)) => {
                            route.gateway = Some(*gateway);
                        }
                        RouteAttribute::Oif(ifd) => {
                            if if_index.is_some_and(|x| x != *ifd) {
                                break; // Skip routes not matching the interface IP
                            }
                            route.out_if_index = Some(*ifd);
                        }
                        RouteAttribute::Priority(priority) => {
                            route.priority = *priority;
                        }
                        _ => {}
                    }
                }
                Ok(route.out_if_index.and(Some(route)))
            }
            _ => Ok(None),
        }
    })
}

/// Retrieves IPv4 addresses associated with network interfaces.
///
/// Optionally filters addresses by a specific interface index.
pub fn get_ipv4_address(if_index: Option<u32>) -> Result<Vec<(Ipv4Addr, u32)>, io::Error> {
    let mut req_msg = AddressMessage::default();
    req_msg.header.family = AddressFamily::Inet;
    let req = NetlinkMessage::from(RouteNetlinkMessage::GetAddress(req_msg));
    netlink(req, |msg| {
        match msg.payload {
            NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewAddress(ref addr_msg)) => {
                if if_index.is_some_and(|x| x != addr_msg.header.index) {
                    return Ok(None); // Skip addresses not matching the interface index
                }
                for attr in addr_msg.attributes.iter() {
                    if let AddressAttribute::Address(IpAddr::V4(ip)) = attr {
                        return Ok(Some((*ip, addr_msg.header.index)));
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    })
}

/// Retrieves a list of all network interfaces (links) from the kernel.
///
/// # How it works
///
/// It constructs a `GetLink` netlink request and sends it using the generic
/// `netlink` function. The response parsing closure processes each `NewLink`
/// message, extracting attributes like interface index, name, MTU, and MAC
/// address to build a `Link` struct for each interface.
pub fn get_links() -> Result<Vec<Link>, io::Error> {
    let req_msg = LinkMessage::default();
    let req = NetlinkMessage::from(RouteNetlinkMessage::GetLink(req_msg));
    netlink(req, |msg| match msg.payload {
        NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewLink(ref link_msg)) => {
            let mut link = Link {
                if_index: link_msg.header.index,
                ..Default::default()
            };
            for attr in link_msg.attributes.iter() {
                match attr {
                    LinkAttribute::IfName(name) => {
                        link.name = name.to_string();
                    }
                    LinkAttribute::Mtu(mtu) => {
                        link.mtu = *mtu;
                    }
                    LinkAttribute::Address(mac) => {
                        if mac.len() == 6 {
                            link.mac = mac[0..6]
                                .try_into()
                                .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
                        } else {
                            return Ok(None);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Some(link))
        }
        _ => Ok(None),
    })
}

/// Finds the default gateway from a list of IPv4 routes.
///
/// The default gateway is identified as the route with a destination prefix of 0
/// and the highest priority.
///
/// # How it works
///
/// It iterates through the provided slice of `Ipv4Route` structs. It looks for
/// routes with a `dest_prefix` of 0, which signifies a default route. Among
/// these, it selects the one with the highest `priority` value (note: in some
/// contexts, lower is better, but here we assume higher value means higher
/// priority as per the existing fold logic). It returns a `Gateway` struct
/// containing the gateway's IP, priority, and output interface index.
pub fn find_default_gateway(routes: &[Ipv4Route]) -> Option<Gateway> {
    routes
        .iter()
        .fold(None, |acc, x| {
            if let Ipv4Route {
                gateway: Some(gw),
                priority,
                out_if_index: Some(oif),
                dest_prefix: 0,
                ..
            } = x
            {
                match acc {
                    None => Some((*gw, *priority, *oif)),
                    Some((_, acc_priority, _)) if acc_priority < *priority => {
                        Some((*gw, *priority, *oif))
                    }
                    _ => acc,
                }
            } else {
                acc
            }
        })
        .map(|(ip, priority, if_index)| Gateway {
            ip,
            if_index,
            priority,
        })
}
