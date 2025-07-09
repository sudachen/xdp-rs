//! # AF_XDP Socket Creation and Configuration
//!
//! ## Purpose
//!
//! This file contains the logic for creating and configuring AF_XDP sockets. It provides
//! a high-level API to set up sockets for transmit-only, receive-only, or
//! bidirectional packet processing, abstracting away many of the low-level details.
//!
//! ## How it works
//!
//! It uses `libc` syscalls to create a raw AF_XDP socket. It then allocates a UMEM
//! (Userspace Memory) region for zero-copy data transfers, configures the necessary
//! rings (TX, RX, Fill, Completion) with appropriate sizes, maps them into memory,
//! and binds the socket to a specific network interface and queue. The logic handles
//! different UMEM and ring configurations based on whether the socket is for TX, RX,
//! or both.
//!
//! ## Main components
//!
//! - `create_socket()`: The core unsafe function that handles the detailed setup logic.
//! - `create_tx_socket()`, `create_rx_socket()`, `create_bi_socket()`: Safe public
//!   functions that wrap `create_socket` for specific use cases.
//! - `setup_umem()`: A helper function to allocate and register the UMEM with the kernel.
//! - `ring_offsets()`: A helper to query the kernel for the memory map offsets of the rings.
//! - `XdpConfig`, `Direction`: Public structs and enums for socket configuration.

use crate::mmap::OwnedMmap;
use crate::ring::{FRAME_COUNT, FRAME_SIZE, Ring, RingType, XdpDesc};
use crate::socket::{_RX, _TX, Inner, RxSocket, TxSocket};
use std::io;
use std::mem::size_of;
use std::os::fd::{FromRawFd as _, OwnedFd};
use std::sync::Arc;

/// Creates one or two sockets for AF_XDP packet processing.
///
/// This is the core function for setting up AF_XDP sockets. It handles UMEM
/// allocation, ring configuration, and binding to a network interface queue.
///
/// # How it works
///
/// 1.  Creates a raw `AF_XDP` socket.
/// 2.  Calls `setup_umem` to create a memory-mapped UMEM region.
/// 3.  Sets the sizes for the Fill, Completion, TX, and RX rings via `setsockopt`.
/// 4.  Retrieves the memory map offsets for the rings from the kernel.
/// 5.  Memory-maps the required rings based on the specified `Direction`.
/// 6.  Binds the socket to the given interface index and queue ID, enabling zero-copy
///     and need-wakeup flags based on the config.
/// 7.  Wraps the components in `TxSocket` and/or `RxSocket` and returns them.
///
/// # Arguments
/// * `if_index` - The index of the network interface to bind to.
/// * `if_queue` - The queue index of the interface to bind to.
/// * `direction` - The desired direction(s) for the socket (`Tx`, `Rx`, or `Both`).
/// * `config` - Optional configuration for zero-copy, huge pages, etc.
///
/// # Returns
/// A tuple `(Option<TxSocket>, Option<RxSocket>)`. The appropriate socket(s) will be
/// `Some` based on the `direction`.
///
/// # Safety
/// This function is unsafe because it directly interfaces with low-level Linux APIs.
/// The caller must ensure the provided parameters are valid.
pub fn create_socket(
    if_index: u32,
    if_queue: u32,
    direction: Direction,
    config: Option<XdpConfig>,
) -> Result<(Option<TxSocket>, Option<RxSocket>), io::Error> {
    let (rx_ring_size, tx_ring_size) = match direction {
        Direction::Tx => (0, FRAME_COUNT), // all frames for outgoing packets
        Direction::Rx => (FRAME_COUNT, 0), // all frames for incoming packets
        Direction::Both => (FRAME_COUNT / 2, FRAME_COUNT / 2), // split frames for both directions
    };

    let (fd, raw_fd) = unsafe {
        let fd = libc::socket(libc::AF_XDP, libc::SOCK_RAW | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        (OwnedFd::from_raw_fd(fd), fd)
    };
    let umem = setup_umem(raw_fd, config.as_ref())?;

    RingType::Fill.set_size(raw_fd, tx_ring_size)?;
    RingType::Completion.set_size(raw_fd, tx_ring_size)?;
    if tx_ring_size > 0 {
        RingType::Tx.set_size(raw_fd, tx_ring_size)?;
    }
    if rx_ring_size > 0 {
        RingType::Rx.set_size(raw_fd, rx_ring_size)?;
    }

    let offsets = ring_offsets(raw_fd)?;

    // Mapping Tx rings in case of Tx and Both direction
    let (c_ring, tx_ring) = if direction == Direction::Rx {
        (Ring::default(), Ring::default())
    } else {
        (
            RingType::Completion.mmap(raw_fd, &offsets, tx_ring_size)?,
            {
                let mut tx_ring: Ring<XdpDesc> =
                    RingType::Tx.mmap(raw_fd, &offsets, tx_ring_size)?;
                tx_ring.fill(0);
                tx_ring
            },
        )
    };

    // Mapping Rx rings in case of Rx and Both direction
    let (rx_ring, f_ring) = if direction == Direction::Tx {
        (Ring::default(), Ring::default())
    } else {
        (RingType::Rx.mmap(raw_fd, &offsets, rx_ring_size)?, {
            let mut f_ring: Ring<u64> = RingType::Fill.mmap(raw_fd, &offsets, rx_ring_size)?;
            f_ring.fill(tx_ring_size as u32);
            f_ring.update_producer(f_ring.len as u32);
            f_ring
        })
    };

    let zero_copy = match config.and_then(|cfg| cfg.zero_copy) {
        Some(true) => libc::XDP_ZEROCOPY,
        Some(false) => libc::XDP_COPY,
        None => 0,
    };

    let need_wakeup = if config.and_then(|cfg| cfg.need_wakeup).unwrap_or(true) {
        libc::XDP_USE_NEED_WAKEUP
    } else {
        0
    };

    let sxdp = libc::sockaddr_xdp {
        sxdp_family: libc::AF_XDP as libc::sa_family_t,
        sxdp_flags: need_wakeup | zero_copy,
        sxdp_ifindex: if_index,
        sxdp_queue_id: if_queue,
        sxdp_shared_umem_fd: 0,
    };

    if unsafe {
        libc::bind(
            raw_fd,
            &sxdp as *const _ as *const libc::sockaddr,
            size_of::<libc::sockaddr_xdp>() as libc::socklen_t,
        ) < 0
    } {
        return Err(io::Error::other(format!(
            "Failed to bind: {}",
            io::Error::last_os_error()
        )));
    }

    // its just owned shared memory and socket descriptor
    // that we can share between Tx and Rx sockets
    // to release it when both are destroyed
    #[allow(clippy::arc_with_non_send_sync)]
    let inner = Arc::new(Inner::new(umem, fd));

    let tx_socket = if direction != Direction::Rx {
        Some(TxSocket::new(Some(inner.clone()), tx_ring, c_ring))
    } else {
        None
    };

    let rx_socket = if direction != Direction::Tx {
        Some(RxSocket::new(Some(inner.clone()), rx_ring, f_ring))
    } else {
        None
    };

    Ok((tx_socket, rx_socket))
}

/// Creates a `TxSocket` for sending packets.
///
/// This is a convenience wrapper around `create_socket` for transmit-only use cases.
///
/// # Arguments
/// * `if_index` - The index of the network interface to use.
/// * `if_queue` - The queue ID of the network interface to use.
/// * `config` - Optional `XdpConfig` to customize the socket.
///
/// # Returns
/// A `Result` containing a `TxSocket` on success, or an `io::Error` on failure.
pub fn create_tx_socket(
    if_index: u32,
    if_queue: u32,
    config: Option<XdpConfig>,
) -> Result<TxSocket, io::Error> {
    let (tx_socket, _) = create_socket(if_index, if_queue, Direction::Tx, config)?;
    tx_socket.ok_or_else(|| io::Error::other("Failed to create Tx socket"))
}

/// Creates an `RxSocket` for receiving packets.
///
/// This is a convenience wrapper around `create_socket` for receive-only use cases.
///
/// # Arguments
/// * `if_index` - The index of the network interface to use.
/// * `if_queue` - The queue ID of the network interface to use.
/// * `config` - Optional `XdpConfig` to customize the socket.
///
/// # Returns
/// A `Result` containing an `RxSocket` on success, or an `io::Error` on failure.
pub fn create_rx_socket(
    if_index: u32,
    if_queue: u32,
    config: Option<XdpConfig>,
) -> Result<RxSocket, io::Error> {
    let (_, rx_socket) = create_socket(if_index, if_queue, Direction::Rx, config)?;
    rx_socket.ok_or_else(|| io::Error::other("Failed to create Rx socket"))
}

/// Creates a pair of sockets (`TxSocket`, `RxSocket`) for bidirectional communication.
///
/// This is a convenience wrapper around `create_socket` for bidirectional use cases.
/// The UMEM frame pool is split between the two sockets.
///
/// # Arguments
/// * `if_index` - The index of the network interface to use.
/// * `if_queue` - The queue ID of the network interface to use.
/// * `config` - Optional `XdpConfig` to customize the sockets.
///
/// # Returns
/// A `Result` containing a tuple of `(TxSocket, RxSocket)` on success, or an `io::Error` on failure.
pub fn create_bi_socket(
    if_index: u32,
    if_queue: u32,
    config: Option<XdpConfig>,
) -> Result<(TxSocket, RxSocket), io::Error> {
    let (tx_socket, rx_socket) = create_socket(if_index, if_queue, Direction::Both, config)?;
    Ok((
        tx_socket.ok_or_else(|| io::Error::other("Failed to create Tx socket"))?,
        rx_socket.ok_or_else(|| io::Error::other("Failed to create Rx socket"))?,
    ))
}

/// Retrieves the memory map offsets for the AF_XDP rings from the kernel.
///
/// This function uses `getsockopt` with `XDP_MMAP_OFFSETS` to query the kernel for
/// the correct offsets of the producer/consumer indices and descriptor arrays for
/// all four rings.
///
/// # Arguments
/// * `raw_fd` - The raw file descriptor of the AF_XDP socket.
pub fn ring_offsets(raw_fd: libc::c_int) -> io::Result<libc::xdp_mmap_offsets> {
    let mut offsets: libc::xdp_mmap_offsets = unsafe { std::mem::zeroed() };
    let mut optlen = size_of::<libc::xdp_mmap_offsets>() as libc::socklen_t;
    unsafe {
        if libc::getsockopt(
            raw_fd,
            libc::SOL_XDP,
            libc::XDP_MMAP_OFFSETS,
            &mut offsets as *mut _ as *mut libc::c_void,
            &mut optlen,
        ) < 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(offsets)
}

/// Allocates and registers the UMEM (Userspace Memory) region with the kernel.
///
/// # How it works
///
/// 1.  It calls `OwnedMmap::mmap` to create a memory-mapped region, optionally
///     backed by huge pages.
/// 2.  It populates an `xdp_umem_reg` struct with the address and size of the UMEM.
/// 3.  It calls `setsockopt` with `XDP_UMEM_REG` to register the UMEM with the
///     kernel, making it available for zero-copy operations.
///
/// # Arguments
/// * `raw_fd` - The raw file descriptor of the AF_XDP socket.
/// * `config` - Optional configuration, used to determine if huge pages should be used.
pub fn setup_umem(raw_fd: libc::c_int, config: Option<&XdpConfig>) -> io::Result<OwnedMmap> {
    let umem = OwnedMmap::mmap(
        FRAME_COUNT * FRAME_SIZE,
        config.and_then(|cfg| cfg.huge_page),
    )
    .map_err(|e| io::Error::other(format!("Failed to allocate UMEM: {}", e)))?;

    let reg = unsafe {
        libc::xdp_umem_reg {
            addr: umem.as_void_ptr() as u64,
            len: umem.len() as u64,
            chunk_size: FRAME_SIZE as u32,
            ..std::mem::zeroed()
        }
    };

    unsafe {
        if libc::setsockopt(
            raw_fd,
            libc::SOL_XDP,
            libc::XDP_UMEM_REG,
            &reg as *const _ as *const libc::c_void,
            size_of::<libc::xdp_umem_reg>() as libc::socklen_t,
        ) < 0
        {
            return Err(io::Error::other(format!(
                "Failed to register UMEM: {}",
                io::Error::last_os_error()
            )));
        }
    }

    Ok(umem)
}

/// Specifies the direction of an AF_XDP socket.
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(i32)]
pub enum Direction {
    /// Transmit-only socket.
    Tx = 0,
    /// Receive-only socket.
    Rx = 1,
    /// Bidirectional socket (both transmit and receive).
    Both = -1,
}

/// Configuration options for creating an AF_XDP socket.
#[derive(Debug, Copy, Clone, Default)]
pub struct XdpConfig {
    /// Enables or disables zero-copy mode.
    ///
    /// - `Some(true)`: Enables `XDP_ZEROCOPY`.
    /// - `Some(false)`: Enables `XDP_COPY`.
    /// - `None`: The kernel's default behavior is used (typically copy mode).
    pub zero_copy: Option<bool>,
    /// Enables or disables huge pages for the UMEM.
    ///
    /// - `Some(true)`: Attempts to use huge pages.
    /// - `Some(false)`: Uses standard page sizes.
    /// - `None`: The implementation default is used (typically standard pages).
    pub huge_page: Option<bool>,
    /// Sets the `XDP_USE_NEED_WAKEUP` flag.
    ///
    /// - `Some(true)`: The flag is set. The application must call `kick()` to wake up the kernel.
    /// - `Some(false)`: The flag is not set. The kernel polls without needing a wakeup call.
    /// - `None`: Defaults to `true`.
    pub need_wakeup: Option<bool>,
}
