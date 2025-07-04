pub mod netlink;
pub mod packet;
pub mod router;
mod xdp;

pub use netlink::{find_default_gateway, get_ipv4_address, get_links, netlink, get_ipv4_routes, get_neighbors};
pub use packet::write_udp_header_for;
pub use router::{Ipv4Route, Neighbor, NextHop, Router};

#[cfg(test)] mod tests;