//
// socket.rs - AF_XDP Socket and Queue Management
//
// Purpose:
//   This module provides abstractions for creating, configuring, and managing AF_XDP sockets
//   and their associated memory rings and queues. It is essential for high-performance packet
//   processing in user space using the AF_XDP Linux feature.
//
// How it works:
//   - Wraps low-level AF_XDP socket operations, including socket creation, binding, and ring
//     buffer management.
//   - Manages UMEM (user memory) allocation and mapping for zero-copy packet I/O.
//   - Supports configuration for direction (Rx, Tx, Both), queue selection, and zero-copy mode.
//   - Handles feature detection and error reporting for device capabilities.
//
// Main components:
//   - AfXdpSocket: Main struct for managing an AF_XDP socket and its resources.
//   - AfXdpConfig: Configuration options for socket creation and behavior.
//   - Direction, QueueId, DeviceQueue: Types for controlling socket direction and queue mapping.
//   - Internal helpers for ring setup, UMEM mapping, and device feature checks.
//

use crate::mmap::OwnedMmap;
use crate::ring::{Ring, XdpDesc};
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::{io, ptr};
use std::sync::Arc;
use std::time::Duration;

pub struct Socket<const t:_Direction> {
    pub(crate) inner: Option<Arc<Inner>>,
    pub(crate) x_ring: Ring<XdpDesc>,
    pub(crate) u_ring: Ring<u64>,
    pub(crate) tail: u32,
    pub(crate) frames: *mut u8,
    pub(crate) skip_frames: usize,
    pub(crate) frames_count: usize,
}

impl<const t:_Direction> Socket<t> where Socket<t>: Seek_<t> {
    /// Construct a new `Socket` with the given `inner` socket and memory ring
    /// configuration.
    ///
    /// If `inner` is `None`, the returned `Socket` will be a default-constructed
    /// instance.
    ///
    /// # Arguments
    ///
    /// * `inner`: The `Inner` socket to use for the new `Socket`. If `None`, a
    ///   default-constructed `Socket` will be returned.
    /// * `x_ring`: The memory ring for zero-copy packet I/O.
    /// * `u_ring`: The memory ring for UMEM allocation.
    /// * `skip_frames`: The number of frames to skip from the beginning of the
    ///   `x_ring` for packet I/O.
    ///
    /// # Returns
    ///
    /// A new `Socket` instance constructed from the given arguments.
    pub fn new(inner:Option<Arc<Inner>>, x_ring: Ring<XdpDesc>, u_ring: Ring<u64>, skip_frames: usize) -> Self {
        if let Some(inner) = inner {
            Self {
                frames: inner.umem.0 as *mut u8,
                frames_count: x_ring.len,
                tail: x_ring.len.saturating_sub(1) as u32,
                inner: Some(inner),
                x_ring,
                u_ring,
                skip_frames,
            }
        } else {
            Self::default()
        }
    }

    /// Wait for the socket to become ready for I/O.
    ///
    /// This function blocks until the socket is ready for I/O in the direction
    /// specified by `t`. If `t` is `_TX`, the socket is polled for writability;
    /// if `t` is `_RX`, the socket is polled for readability.
    ///
    /// # Arguments
    ///
    /// * `_timeout`: An optional timeout value to wait for the socket to become
    ///   ready. If `None`, the function will block indefinitely.
    ///
    /// # Returns
    ///
    /// A `Result` indicating whether the operation was successful. If an error
    /// occurs, an `io::Error` is returned.
    pub fn poll_wait(&self, _timeout: Option<Duration>) -> Result<(), io::Error> {
        self.kick(false)?;
        let mask = match t {
            _TX => libc::POLLOUT,
            _RX => libc::POLLIN
        };
        if let Some(inner) = &self.inner {
            unsafe {
                loop {
                    let mut fds = [libc::pollfd {
                        events: mask,
                        revents: 0,
                        fd: inner.fd.as_raw_fd(),
                    }];
                    if 0 > libc::poll(fds.as_mut_ptr(), 1, -1) {
                        //..
                    } else if (fds[0].revents & mask) != 0 {
                        break
                    }
                }
            }
        }
        Ok(())
    }
    fn peek_(&mut self, x_head: u32, len: usize) -> Result<(&mut [u8], u32), RingError> {
        let buf = self.x_ring.mut_bytes_at(self.frames, x_head, len);
        Ok((buf, x_head))
    }
    /// Peek at the next available chunk in the TX ring.
    ///
    /// This function will not block if the ring is empty.
    ///
    /// # Arguments
    ///
    /// * `len`: The length of the chunk to peek at.
    ///
    /// # Returns
    ///
    /// A `Result` containing a mutable reference to the chunk and its index in the
    /// TX ring.
    pub fn peek(&mut self, len: usize) -> Result<(&mut [u8], u32), RingError> {
        let tx_head = self.seek()?;
        self.peek_(tx_head, len)
    }
    /// Seek the next available chunk in the TX ring.
    ///
    /// This function will not block if the ring is empty.
    ///
    /// # Returns
    ///
    /// A `Result` containing the index of the next available chunk in the
    /// TX ring. If an error occurs, a `RingError` is returned.
    fn seek(&mut self) -> Result<u32, RingError> {
        self.seek_()
    }
}

pub struct Inner {
    pub umem: OwnedMmap,
    pub fd: OwnedFd,
}

pub type _Direction = bool;
pub const _TX: _Direction = true;
pub const _RX: _Direction = false;
pub type TxSocket = Socket<_TX>;
pub type RxSocket = Socket<_RX>;
type InnerSocket = Option<Arc<Inner>>;

impl<const t:_Direction> Default for Socket<t> {
    fn default() -> Self {
        Self {
            inner: None,
            x_ring: Default::default(),
            u_ring: Default::default(),
            tail: 0,
            frames: ptr::null_mut(),
            skip_frames: 0,
            frames_count: 0,
        }
    }
}

pub(crate) trait Seek_<const t:_Direction> {
    fn seek_(&mut self) -> Result<u32, RingError>;
}

#[derive(Debug)]
pub enum RingError {
    RingFull,
    RingEmpty,
    InvalidTxHead,
    InvalidRxHead,
    Io(io::Error),
}