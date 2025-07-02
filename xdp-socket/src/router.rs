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
//   - Ipv4Route: Represents a single IPv4 route entry (prefix, gateway, output interface).
//   - Neighbor: Represents a single ARP entry (IP, MAC, interface).
//   - Gateway: Represents a default gateway.
//

pub use crate::netlink::{Ipv4Route, Neighbor, get_ipv4_routes, get_neighbors};
use ipnet::Ipv4Net;
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
        if let Some(neighbour) = self.neighbors.get(&dest_ip) {
            return Some(NextHop {
                ip_addr: dest_ip,
                mac_addr: Some(neighbour.mac),
            });
        }
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
    pub neighbors: HashMap<Ipv4Addr, Neighbor>,
    pub routes: PrefixMap<Ipv4Net, Ipv4Route>,
}

#[derive(Clone, Debug)]
pub struct NextHop {
    pub ip_addr: Ipv4Addr,
    pub mac_addr: Option<[u8; 6]>,
}
