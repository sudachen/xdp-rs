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
#![allow(private_bounds)]
#![allow(non_upper_case_globals)]

use crate::mmap::OwnedMmap;
use crate::ring::{Ring, XdpDesc};
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::{io, ptr};
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;

/// A high-level interface for an AF_XDP socket.
///
/// This struct is generic over the `_Direction` const parameter, which determines
/// whether the socket is for sending (`_TX`) or receiving (`_RX`).
pub struct Socket<const t: _Direction> {
    /// The inner shared state, including the file descriptor and UMEM.
    pub(crate) _inner: Option<Arc<Inner>>,
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
    /// -
    pub(crate) raw_fd: libc::c_int,
}

/// An error that can occur during ring operations.
#[derive(Debug)]
pub enum RingError {
    /// The ring is full, and no more descriptors can be produced.
    RingFull,
    /// The ring is empty, and no descriptors can be consumed.
    RingEmpty,
    /// Not enough descriptors or frames are available for the requested operation.
    NotAvailable,
    /// An invalid index was used to access a ring descriptor.
    InvalidIndex,
    /// The provided data length exceeds the available space in a UMEM frame.
    InvalidLength,
    /// An underlying I/O error occurred.
    Io(io::Error),
}

impl Display for RingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RingError::RingFull => write!(f, "Ring is full"),
            RingError::RingEmpty => write!(f, "Ring is empty"),
            RingError::NotAvailable => write!(f, "Not enough available frames"),
            RingError::InvalidIndex => write!(f, "Invalid index for ring access"),
            RingError::InvalidLength => write!(f, "Invalid length for ring access"),
            RingError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}


impl<const t: _Direction> Socket<t>
where
    Socket<t>: Seek_<t> + Commit_<t> + Send,
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
    ) -> Self {
        if let Some(inner) = inner {
            let frames = inner.umem.0 as *mut u8;
            let raw_fd = inner.fd.as_raw_fd();
            Self {
                frames,
                available: x_ring.len as u32,
                producer: 0,
                consumer: 0,
                raw_fd,
                _inner: Some(inner),
                x_ring,
                u_ring,
            }
        } else {
            Self::default()
        }
    }
    
    /// Ensures that at least one descriptor is available for the next operation and
    /// returns the total number of available descriptors.
    ///
    /// For a `TxSocket`, this may involve reclaiming completed descriptors from the
    /// Completion Ring. For an `RxSocket`, this checks for newly received packets.
    ///
    /// # Returns
    ///
    /// A `Result` containing the total number of available descriptors, or a
    /// `RingError` if the operation fails.
    #[inline]
    pub fn seek(&mut self) -> Result<usize, RingError> {
        self.seek_(1)
    }

    /// Ensures that at least `count` descriptors are available for the next operation
    /// and returns the total number of available descriptors.
    ///
    /// For a `TxSocket`, this may involve reclaiming completed descriptors from the
    /// Completion Ring. For an `RxSocket`, this checks for newly received packets.
    ///
    /// # Arguments
    ///
    /// * `count` - The desired number of available descriptors.
    ///
    /// # Returns
    ///
    /// A `Result` containing the total number of available descriptors, or a
    /// `RingError` if the operation fails.
    #[inline]
    pub fn seek_n(&mut self, count: usize) -> Result<usize, RingError> {
        self.seek_(count)
    }

    /// Commits one descriptor, making it available to the kernel.
    ///
    /// For a `TxSocket`, this signals to the kernel that a packet written to the
    /// corresponding UMEM frame is ready to be sent.
    ///
    /// For an `RxSocket`, this returns a UMEM frame to the kernel's Fill Ring after
    /// the application has finished processing the received packet, making the frame
    /// available for new packets.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or a `RingError` on failure.
    #[inline]
    pub fn commit(&mut self) -> Result<(), RingError> {
        self.commit_(1)
    }

    /// Commits `n` descriptors, making them available to the kernel.
    ///
    /// For a `TxSocket`, this signals to the kernel that `n` packets are ready to be sent.
    /// For an `RxSocket`, this returns `n` UMEM frames to the kernel's Fill Ring.
    ///
    /// # Arguments
    ///
    /// * `n` - The number of descriptors to commit.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or a `RingError` on failure.
    #[inline]
    pub fn commit_n(&mut self, n: usize) -> Result<(), RingError> {
        self.commit_(n)
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

// socket refers to shared mapped memory owned by _inner and rings
//  so all pointers can be safely send over threads
//  until mapped memory is alive
unsafe impl<const t:_Direction> Send for Socket<t> {}


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
            _inner: None,
            x_ring: Default::default(),
            u_ring: Default::default(),
            available: 0,
            producer: 0,
            consumer: 0,
            frames: ptr::null_mut(),
            raw_fd: 0
        }
    }
}

/// A trait for direction-specific seeking logic (TX vs. RX).
pub(crate) trait Seek_<const t: _Direction> {
    fn seek_(&mut self, count: usize) -> Result<usize, RingError>;
}

pub(crate) trait Commit_<const t: _Direction> {
    fn commit_(&mut self, count: usize) -> Result<(), RingError>;
}

/// Holds the owned components of an XDP socket that can be shared.
pub(crate) struct Inner {
    /// The memory-mapped UMEM region.
    umem: OwnedMmap,
    /// The owned file descriptor for the AF_XDP socket.
    fd: OwnedFd,
}

impl Inner {
    /// Constructs a new `Inner` with the given UMEM and file descriptor.
    pub(crate) fn new(umem: OwnedMmap, fd: OwnedFd) -> Self {
        Self { umem, fd }
    }
}
