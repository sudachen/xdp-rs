//! # IPv4 Routing and Neighbor Table Cache
//!
//! ## Purpose
//!
//! This module provides an efficient, cached interface to the system's IPv4 routing and
//! neighbor (ARP) tables for a specific network interface. It enables fast next-hop
//! resolution (gateway IP and destination MAC address) for outgoing packets.
//!
//! ## How it works
//!
//! It uses the functions from the `netlink` module to fetch routes and neighbors from the
//! kernel. It caches this data in efficient in-memory structures: a `PrefixMap` (a prefix
//! trie) for routes, enabling fast longest-prefix-match lookups, and a `HashMap` for
//! neighbors. The `route` method performs lookups against this cache to find the next hop.
//!
//! ## Main components
//!
//! - `Router`: The main struct that holds the cached routing and neighbor tables.
//! - `route()`: Performs a next-hop lookup for a given destination IP address.
//! - `refresh()`: Updates the cache by re-querying the kernel's tables via netlink.
//! - `NextHop`: A struct representing the result of a successful route lookup.

pub use crate::util::netlink::{Ipv4Route, Neighbor, get_ipv4_routes, get_neighbors};
use ipnet::Ipv4Net;
use prefix_trie::PrefixMap;
use std::collections::HashMap;
use std::io;
use std::net::Ipv4Addr;

impl Router {
    /// Creates a new `Router` for a specific network interface.
    ///
    /// # Arguments
    /// * `if_index` - The index of the network interface to manage routes for.
    pub fn new(if_index: u32) -> Self {
        Router {
            if_index,
            routes: PrefixMap::new(),
            neighbors: HashMap::new(),
        }
    }

    /// Finds the next-hop information for a given destination IPv4 address.
    ///
    /// # How it works
    ///
    /// It first performs a longest-prefix-match (LPM) lookup in the cached `routes` table.
    /// If a route is found, it determines the next-hop IP (either the gateway or the destination itself).
    /// It then looks up the MAC address for that next-hop IP in the `neighbors` cache.
    /// If no route is found via LPM, it attempts a direct lookup in the neighbor cache for the destination IP.
    /// Returns a `NextHop` struct containing the next-hop IP and MAC address if successful.
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

    /// Refreshes the router's cached tables from the kernel.
    ///
    /// # How it works
    ///
    /// It calls `get_ipv4_routes` and `get_neighbors` from the `netlink` module to fetch
    /// the latest data from the kernel for the router's interface. It then rebuilds the
    /// internal `routes` prefix map and `neighbors` hash map with the new data.
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

/// Manages a cached view of the system's IPv4 routing and neighbor tables.
///
/// It holds the routing table in a prefix trie for efficient longest-prefix-match
/// lookups and the neighbor (ARP) table in a hash map.
pub struct Router {
    /// The network interface index this router is bound to.
    pub if_index: u32,
    /// A cache of neighbor (ARP) entries, mapping IP addresses to `Neighbor` structs.
    pub neighbors: HashMap<Ipv4Addr, Neighbor>,
    /// A prefix trie (`PrefixMap`) for efficient longest-prefix-match lookups on routes.
    pub routes: PrefixMap<Ipv4Net, Ipv4Route>,
}

/// Represents the next hop for an outgoing packet.
#[derive(Clone, Debug)]
pub struct NextHop {
    /// The IP address of the next hop (either the final destination or a gateway).
    pub ip_addr: Ipv4Addr,
    /// The MAC address of the next hop, if resolved.
    pub mac_addr: Option<[u8; 6]>,
}
