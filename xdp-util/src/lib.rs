//!
//! # XDP Utility Library
//!
//! This module provides utility functions and helpers for XDP socket operations, networking,
//! and packet processing. It includes routines for interacting with netlink, handling packet
//! headers, managing routing information, working with XDP programs, and retrieving MAC
//! addresses by interface index. The utilities facilitate low-level networking tasks and
//! abstract common operations needed by other XDP modules.
//!

pub mod netlink;
pub mod packet;
pub mod router;
pub mod xdp_prog;
pub mod mac_by_ifindex;

pub use netlink::{find_default_gateway, get_ipv4_address, get_links, netlink, get_ipv4_routes, get_neighbors};
pub use packet::write_udp_header_for;
pub use router::{Ipv4Route, Neighbor, NextHop, Router};
pub use xdp_prog::{OwnedXdpProg, xdp_features, xdp_attach_program};
pub use mac_by_ifindex::mac_by_ifindex;
