//! # XDP Socket Implementation
//!
//! ## Purpose
//!
//! This file implements the `Socket` struct, which provides a high-level interface for
//! interacting with XDP sockets. It supports both sending (TX) and receiving (RX) of
//! packets with high performance through zero-copy data transfers.
//!
//! ## How it works
//!
//! The `Socket` utilizes two main components for its operation: a UMEM (Userspace Memory)
//! region and associated rings for communication with the kernel. The UMEM is a memory-mapped
//! area shared between the userspace application and the kernel, which allows for zero-copy
//! packet processing.
//!
//! - For sending packets (TX), the application writes packet data directly into frames within
//!   the UMEM and then pushes descriptors to the TX ring, signaling the kernel to send them.
//! - For receiving packets (RX), the application provides the kernel with descriptors pointing
//!   to free frames in the UMEM via the Fill ring. The kernel writes incoming packet data
//!   into these frames and notifies the application through the RX ring.
//!
//! ## Main components
//!
//! - `Socket<const t:_Direction>`: The primary struct representing an XDP socket. It is
//!   generic over the direction (TX or RX) to provide a type-safe API for each use case.
//! - `Ring<T>`: A generic ring buffer implementation that is used for the TX/RX rings and
//!   the Fill/Completion rings for UMEM.
//! - `Inner`: A struct that holds the owned file descriptor for the XDP socket and the
//!   memory-mapped UMEM region.
//! - `TxSocket` and `RxSocket`: Type aliases for `Socket<true>` and `Socket<false>`
//!   respectively, providing a more intuitive API for users.

#![allow(private_interfaces)]
use crate::mmap::OwnedMmap;
use crate::ring::{Ring, XdpDesc};
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::{io, ptr};
use std::sync::Arc;
use std::time::Duration;

/// A high-level interface for an AF_XDP socket.
///
/// This struct is generic over the `_Direction` const parameter, which determines
/// whether the socket is for sending (`_TX`) or receiving (`_RX`).
pub struct Socket<const t: _Direction> {
    /// The inner shared state, including the file descriptor and UMEM.
    pub(crate) inner: Option<Arc<Inner>>,
    /// The primary ring for sending (TX) or receiving (RX) descriptors.
    pub(crate) x_ring: Ring<XdpDesc>,
    /// The UMEM-associated ring: Completion Ring for TX, Fill Ring for RX.
    pub(crate) u_ring: Ring<u64>,
    /// The number of available descriptors in the `x_ring`.
    pub(crate) available: u32,
    /// The cached producer index for the `x_ring`.
    pub(crate) producer: u32,
    /// The cached consumer index for the `x_ring`.
    pub(crate) consumer: u32,
    /// A raw pointer to the start of the UMEM frames area.
    pub(crate) frames: *mut u8,
    /// The number of frames at the start of the UMEM to skip.
    pub(crate) skip_frames: usize,
    /// The total number of frames in the UMEM.
    pub(crate) frames_count: usize,
}

/// An error that can occur during ring operations.
#[derive(Debug)]
pub enum RingError {
    /// The ring is full, and no more items can be added.
    RingFull,
    /// The ring is empty, and no items can be retrieved.
    RingEmpty,
    /// The TX ring head is in an invalid state.
    NotAvailable,
    /// The RX ring head is in an invalid state.
    InvalidRxHead,
    /// An underlying I/O error occurred.
    Io(io::Error),
}

impl<const t: _Direction> Socket<t>
where
    Socket<t>: Seek_<t>,
{
    /// Constructs a new `Socket`.
    ///
    /// This function initializes a socket for either sending or receiving based on the
    /// generic const `t`. For TX sockets, it pre-fills the TX ring with descriptors
    /// pointing to UMEM frames. For RX sockets, it pre-fills the Fill ring to provide
    /// the kernel with available frames for incoming packets.
    ///
    /// # Arguments
    ///
    /// * `inner` - The shared inner socket state (file descriptor, UMEM).
    /// * `x_ring` - The TX or RX ring.
    /// * `u_ring` - The Completion or Fill ring.
    /// * `skip_frames` - The number of frames to skip at the start of the UMEM.
    pub(crate) fn new(
        inner: Option<Arc<Inner>>,
        mut x_ring: Ring<XdpDesc>,
        mut u_ring: Ring<u64>,
        skip_frames: usize,
    ) -> Self {
        if let Some(inner) = inner {
            match t {
                _TX => {
                    // all frames available for sending packets
                    x_ring.fill(skip_frames as u32);
                }
                _RX => {
                    // all frames available for receiving packets
                    u_ring.fill(skip_frames as u32);
                    u_ring.update_producer(u_ring.len as u32);
                }
            };
            Self {
                frames: inner.umem.0 as *mut u8,
                frames_count: x_ring.len,
                available: x_ring.len as u32,
                producer: 0,
                consumer: 0,
                inner: Some(inner),
                x_ring,
                u_ring,
                skip_frames,
            }
        } else {
            Self::default()
        }
    }

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
    pub fn poll_wait(&self, _timeout: Option<Duration>) -> Result<(), io::Error> {
        self.kick()?;
        let mask = match t {
            _TX => libc::POLLOUT,
            _RX => libc::POLLIN,
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
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Internal helper for peeking at a chunk in the ring without advancing the head.
    ///
    /// This function is used by `peek` and `peek_n` to peek at a chunk in the ring
    /// without advancing the head.
    ///
    /// # Arguments
    ///
    /// * `x_head` - The head index of the descriptor to peek at.
    /// * `len` - The length of the chunk to peek at.
    ///
    /// # Returns
    ///
    /// A `Result` containing a mutable slice into the UMEM if successful, or a
    /// `RingError` if the operation fails.
    fn peek_(&mut self, x_head: u32, len: usize) -> Result<&mut [u8], RingError> {
        let buf = self.x_ring.mut_bytes_at(self.frames, x_head, len);
        Ok(buf)
    }

    /// Peeks at the next available chunk in the ring without advancing the head.
    ///
    /// This function finds the next available descriptor using `seek` and returns a
    /// mutable slice into the UMEM for writing (TX) or reading (RX).
    ///
    /// # Arguments
    ///
    /// * `len` - The desired length of the chunk.
    ///
    /// # Returns
    ///
    /// A `Result` containing a mutable byte slice and its corresponding descriptor index.
    pub fn peek(&mut self, len: usize) -> Result<&mut [u8], RingError> {
        let (tx_head,_) = self.seek_()?;
        self.peek_(tx_head, len)
    }

    /// Returns the number of available frames in the ring.
    ///
    /// This function seeks to the next available descriptor in the ring using `seek_`
    /// and returns the number of available frames.
    ///
    /// # Returns
    ///
    /// A `Result` containing the number of available frames, or a `RingError` if
    /// the operation fails.
    pub fn seek(&mut self) -> Result<usize, RingError> {
        let (_, available) = self.seek_()?;
        Ok(available as usize)
    }

    /// Peeks at the `index`-th available chunk in the ring without advancing the head.
    ///
    /// This function finds the `index`-th descriptor in the range of AVAILABLE descriptors
    /// and returns a mutable slice into the UMEM for writing (TX) or reading (RX).
    ///
    /// # Arguments
    ///
    /// * `len` - The desired length of the chunk.
    /// * `index` - The index of the chunk in the available frames.
    ///
    /// # Returns
    ///
    /// A `Result` containing a mutable byte slice and its corresponding descriptor index.
    ///
    pub fn peek_n(&mut self, len: usize, index: usize) -> Result<&mut [u8], RingError> {
        debug_assert!(len < self.frame_size() as usize, "Length exceeds frame size");
        debug_assert!(self.available > index as u32, "Index out of bounds for available frames");
        let x_head = (self.producer + index as u32) & self.x_ring.mod_mask;
        self.peek_(x_head, len)
    }

    /// Returns the size of a single frame in the UMEM.
    ///
    /// # Returns
    ///
    /// The size of a single frame in the UMEM in bytes.
    #[inline]
    pub fn frame_size(&self) -> usize {
        self.x_ring.frame_size() as usize
    }
}

/// A boolean flag indicating the direction of the socket (`true` for TX, `false` for RX).
pub type _Direction = bool;

/// Constant representing the Transmit (TX) direction.
pub const _TX: _Direction = true;

/// Constant representing the Receive (RX) direction.
pub const _RX: _Direction = false;

/// A type alias for a socket configured for sending packets.
pub type TxSocket = Socket<_TX>;

/// A type alias for a socket configured for receiving packets.
pub type RxSocket = Socket<_RX>;

impl<const t: _Direction> Default for Socket<t> {
    fn default() -> Self {
        Self {
            inner: None,
            x_ring: Default::default(),
            u_ring: Default::default(),
            available: 0,
            producer: 0,
            consumer: 0,
            frames: ptr::null_mut(),
            skip_frames: 0,
            frames_count: 0,
        }
    }
}

/// A trait for direction-specific seeking logic (TX vs. RX).
pub(crate) trait Seek_<const t: _Direction> {
    /// Finds the next available descriptor in the ring.
    fn seek_(&mut self) -> Result<(u32,u32), RingError>;
}

/// Holds the owned components of an XDP socket that can be shared.
pub(crate) struct Inner {
    /// The memory-mapped UMEM region.
    pub(crate) umem: OwnedMmap,
    /// The owned file descriptor for the AF_XDP socket.
    pub(crate) fd: OwnedFd,
}
