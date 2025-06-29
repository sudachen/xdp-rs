//
// router.rs - IPv4 Routing and Neighbor Table Management for AF_XDP
//
// Purpose:
//   This module provides efficient access to the IPv4 routing table and neighbor (ARP) cache
//   for a given network interface. It enables fast next-hop resolution (gateway and MAC address)
//   for outgoing packets, which is essential for high-performance networking with AF_XDP.
//
// How it works:
//   - Uses netlink (via netlink_packet_core and netlink_packet_route) to fetch and parse kernel
//     routing and neighbor tables.
//   - Caches routes and neighbors in fast in-memory structures (prefix trie for routes, hashmap
//     for neighbors).
//   - Provides API to refresh the cache, look up next hops, and retrieve gateway or neighbor
//     information for a given destination.
//
// Main components:
//   - Router: Main struct, maintains per-interface routing and neighbor cache, provides lookup
//     and refresh methods.
//   - Ipv4Route: Represents a single IPv4 route entry (prefix, gateway, output interface).
//   - Neighbor: Represents a single ARP entry (IP, MAC, interface).
//   - Gateway: Represents a default gateway.
//   - Core functions: get_ipv4_routes, get_neighbors, find_default_gateway, netlink (generic
//     netlink query helper).
//

use ipnet::Ipv4Net;
use netlink_packet_core::{
    NLM_F_DUMP, NLM_F_REQUEST, NetlinkDeserializable, NetlinkMessage, NetlinkPayload,
    NetlinkSerializable,
};
use netlink_packet_route::{
    AddressFamily, RouteNetlinkMessage,
    neighbour::{NeighbourAddress, NeighbourAttribute, NeighbourMessage},
    route::{RouteAddress, RouteAttribute, RouteMessage},
};
use netlink_sys::{Socket, SocketAddr};
use prefix_trie::PrefixMap;
use std::collections::HashMap;
use std::io;
use std::net::Ipv4Addr;

impl Router {
    pub fn new(if_index: u32) -> Self {
        Router {
            if_index,
            routes: PrefixMap::new(),
            neighbors: HashMap::new(),
        }
    }
    pub fn route(&mut self, dest_ip: Ipv4Addr) -> Option<NextHop> {
        let dest_net = Ipv4Net::from(dest_ip);
        if let Some((_, route)) = self.routes.get_lpm(&dest_net) {
            let ip = route.gateway.unwrap_or(dest_ip);
            if let Some(neighbour) = self.neighbors.get(&ip) {
                return Some(NextHop {
                    ip_addr: ip,
                    mac_addr: Some(neighbour.mac),
                });
            }
        };
        None
    }
    pub fn refresh(&mut self) -> Result<(), io::Error> {
        let mut routes = get_ipv4_routes(Some(self.if_index))?;
        let neighbors = get_neighbors(Some(self.if_index))?;
        let mut prefix_map = PrefixMap::new();
        for route in routes.drain(..) {
            let dest_net = Ipv4Net::new(route.destination, route.dest_prefix).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Invalid destination prefix")
            })?;
            prefix_map.insert(dest_net, route);
        }
        self.neighbors = neighbors.into_iter().map(|n| (n.ip, n)).collect();
        self.routes = prefix_map;
        Ok(())
    }
}

pub struct Router {
    pub if_index: u32,
    neighbors: HashMap<Ipv4Addr, Neighbor>,
    routes: PrefixMap<Ipv4Net, Ipv4Route>,
}

pub struct NextHop {
    pub ip_addr: Ipv4Addr,
    pub mac_addr: Option<[u8; 6]>,
}

#[derive(Clone, Copy, Debug)]
pub struct Ipv4Route {
    pub dest_prefix: u8,
    pub destination: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub out_if_index: Option<u32>,
    pub priority: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Neighbor {
    pub ip: Ipv4Addr,
    pub mac: [u8; 6],
    pub if_index: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Gateway {
    pub ip: Ipv4Addr,
    pub priority: u32,
    pub if_index: u32,
}

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

#[test]
fn test_list_routes() {
    let all_routes = get_ipv4_routes(None).unwrap();
    let gw = find_default_gateway(&all_routes).unwrap();
    println!("default GW: {:#?} ", gw);
    let routes = get_ipv4_routes(Some(gw.if_index)).unwrap();
    println!("{:#?}", routes);
}

#[test]
fn list_neighbors() {
    let all_routes = get_ipv4_routes(None).unwrap();
    let gw = find_default_gateway(&all_routes).unwrap();
    let neighbors = get_neighbors(Some(gw.if_index)).unwrap();
    for n in neighbors {
        println!("Neighbor: {:#?}", n);
    }
}
