#![doc = include_str!("../README.md")]

// Public modules and re-exports
pub mod create_socket;
pub mod mmap;
pub mod ring;
pub mod socket;
pub mod util;

pub use create_socket::{
    create_bi_socket, create_rx_socket, create_socket, create_tx_socket, Direction, XdpConfig,
};
pub use socket::Socket;

// Internal modules, hidden from documentation
#[doc(hidden)]
pub mod commit;
#[doc(hidden)]
pub mod kick;
#[doc(hidden)]
pub mod seek;
#[doc(hidden)]
pub mod send;
