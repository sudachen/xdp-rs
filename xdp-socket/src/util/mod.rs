//! # Utility Module for Network Operations
//!
//! ## Purpose
//!
//! This file serves as the entry point for the `util` module. It organizes and
//! publicly exports various networking utilities required by the `xdp-socket` library
//! and potentially useful for applications using it.
//!
//! ## How it works
//!
//! It declares the sub-modules (`netlink`, `packet`, `router`, `xdp`) using `pub mod`
//! and `mod` statements. It then uses `pub use` to re-export the most important
//! functions and structs from these sub-modules, creating a consolidated and easy-to-use
//! public API for the `util` module.
//!
//! ## Main components
//!
//! - Module declarations: Brings the utility sub-modules into the crate's scope.
//! - Public re-exports (`pub use`): Exposes functionalities like route lookups,
//!   packet header creation, and netlink queries to the rest of the crate.

pub mod netlink;
pub mod packet;
pub mod router;
pub mod xdp_prog;

pub use netlink::{find_default_gateway, get_ipv4_address, get_links, netlink, get_ipv4_routes, get_neighbors};
pub use packet::write_udp_header_for;
pub use router::{Ipv4Route, Neighbor, NextHop, Router};
pub use xdp_prog::{OwnedXdpProg, xdp_features, xdp_attach_program};

