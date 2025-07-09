//!
//! # xdp-socket
//!
//! A minimal and efficient Rust implementation of AF_XDP sockets for high-performance
//! packet processing. This crate provides a low-level, transparent API to interact with
//! XDP sockets, enabling direct, zero-copy access to network interfaces for both transmit
//! and receive operations.
//!
//! ## Features
//!
//! - Simple and flexible API for AF_XDP sockets
//! - Support for both TX and RX directions
//! - UMEM management and ring buffer handling
//! - Utilities for polling, sending, and kernel wakeup
//! - Designed for integration with async runtimes or custom event loops
//!
//! This crate is intended for developers building fast packet processing applications or
//! custom networking stacks in Rust.
//!
//! ## Main Components
//!
//! - [`Socket`]: The main type representing an AF_XDP socket, parameterized by direction
//!   (TX or RX). Provides methods for sending, receiving, and managing descriptors.
//! - [`UMEM`]: User memory region for zero-copy packet buffers, shared with the kernel.
//! - Ring Buffers: Fill, Completion, TX, and RX rings for packet flow control and
//!   synchronization with the kernel.
//! - [`PollWaitExt`]: Trait for blocking until the socket is ready for I/O.
//! - [`SendExt`]: Trait for high-level, ergonomic packet sending on transmit sockets.
//!
//! ## Descriptor Flow: seek → peek → commit → kick
//!
//! The typical workflow for both sending and receiving packets involves the following steps:
//!
//! 1. **seek**: Reserve one or more descriptors in the ring buffer, ensuring space for
//!    packet data.
//! 2. **peek**: Access the reserved UMEM region for reading (RX) or writing (TX) packet data.
//! 3. **commit**: Mark the descriptor as ready for the kernel (TX) or for user processing (RX).
//! 4. **kick**: Notify the kernel to process the descriptors if the ring requires wakeup.
//!
//! This flow enables efficient, lock-free packet exchange between user space and the kernel,
//! minimizing syscalls and maximizing throughput. For TX, you write data after seek/peek,
//! then commit and kick. For RX, you seek/peek to fetch data, then commit to release the
//! descriptor back to the kernel.
//!

// Public modules and re-exports
pub mod create;
pub mod mmap;
pub mod ring;
pub mod socket;

pub use create::{
    Direction, XdpConfig, create_bi_socket, create_rx_socket, create_socket, create_tx_socket,
};
pub use socket::Socket;

// Internal modules, hidden from documentation
#[doc(hidden)]
pub mod commit;
#[doc(hidden)]
pub mod kick;
pub mod peek;
pub mod poll;
#[doc(hidden)]
pub mod seek;
#[doc(hidden)]
pub mod send;

pub use {poll::PollWaitExt, send::SendExt};
