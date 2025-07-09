//!
//! # XDP Socket Poll Utilities
//!
//! This file provides utilities for polling XDP socket file descriptors. It enables waiting
//! for readiness events such as readability or writability, which is essential for efficient
//! I/O in high-performance networking scenarios. The utilities abstract low-level polling
//! mechanisms and can be used to integrate XDP sockets with event-driven or blocking code.
//!
//! Since poll_wait is not a part of core API,
//!   you have to import PollWait trait to enable it.
//!
//! ## How it works
//!
//! The `poll_wait` method blocks the current thread until the socket's file descriptor
//! becomes ready for I/O. It uses `poll` to wait for the socket's readiness event,
//! which depends on the socket direction:
//! - For transmit sockets (`_TX`), it waits for the socket to be writable (`POLLOUT`).
//! - For receive sockets (`_RX`), it waits for the socket to be readable (`POLLIN`).
//!
//! ## Main components
//!
//! - `impl PollWait<_TX>`: An implementation block for the transmit socket.
//! - `impl PollWait<_RX>`: An implementation block for the receive socket.
//! - `poll_wait()`: A method to block until a socket becomes ready for I/O.
//!

use std::io;
use std::time::Duration;
use crate::socket::{_Direction, Socket, _TX, _RX, Commit_};

/// A trait for polling XDP sockets for readiness events.
///
/// This trait provides the `poll_wait` method, which blocks until the socket's file
/// descriptor becomes ready for I/O. The readiness event depends on the socket direction:
/// - For transmit sockets (`_TX`), it waits for the socket to be writable (`POLLOUT`).
/// - For receive sockets (`_RX`), it waits for the socket to be readable (`POLLIN`).
///
/// # Type Parameters
///
/// * `t` - The direction of the socket (`_TX` or `_RX`).
///
/// # Example
///
/// ```rust
/// use xdp_socket::PollWaitExt;
/// let socket = ...; // your Socket<_TX> or Socket<_RX>
/// socket.poll_wait(Some(std::time::Duration::from_secs(1)))?;
/// ```
pub trait PollWaitExt<const t:_Direction> {
    fn poll_wait(&self, _timeout: Option<Duration>) -> Result<(), io::Error>;
}

impl<const t:_Direction> PollWaitExt<t> for Socket<t> where Socket<t>: Commit_<t> {
    /// Waits for the socket to become ready for I/O, blocking until an event occurs.
    ///
    /// This function uses `poll` to wait for the socket's file descriptor to become
    /// ready. For a `TxSocket`, it waits for `POLLOUT` (writable). For an `RxSocket`,
    /// it waits for `POLLIN` (readable).
    ///
    /// # Arguments
    ///
    /// * `_timeout` - An optional timeout. If `None`, it blocks indefinitely.
    ///
    /// # Returns
    ///
    /// An `io::Result` indicating success or failure.
    fn poll_wait(&self, _timeout: Option<Duration>) -> Result<(), io::Error> {
        self.kick()?;
        let mask = match t {
            _TX => libc::POLLOUT,
            _RX => libc::POLLIN,
        };
        unsafe {
            loop {
                let mut fds = [libc::pollfd {
                    events: mask,
                    revents: 0,
                    fd: self.raw_fd,
                }];
                if 0 > libc::poll(fds.as_mut_ptr(), 1, -1) {
                    //..
                } else if (fds[0].revents & mask) != 0 {
                    break;
                }
            }
        }
        Ok(())
    }
}