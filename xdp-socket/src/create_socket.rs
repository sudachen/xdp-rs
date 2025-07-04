use crate::mmap::OwnedMmap;
use crate::ring::{FRAME_COUNT, FRAME_SIZE, Ring, RingType};
use crate::socket::{Inner, RxSocket, TxSocket};
use std::io;
use std::os::fd::{FromRawFd as _, OwnedFd};
use std::sync::Arc;

/// Create a socket for AF_XDP packet processing.
///
/// `if_index` specifies the index of the network interface to bind to.
/// `if_queue` specifies the queue index of the interface to bind to.
/// `direction` specifies the direction of the socket.
/// `config` specifies the configuration options for the socket.
///
/// Returns a tuple of `(Option<TxSocket>, Option<RxSocket>)`, where:
/// - `TxSocket` is `Some` if `direction` is `Direction::Tx` or `Direction::Both`.
/// - `RxSocket` is `Some` if `direction` is `Direction::Rx` or `Direction::Both`.
///
/// If an error occurs during socket creation, returns an `io::Error`.
///
/// # Safety
///
/// This function is unsafe because it creates a socket using the `AF_XDP` socket family,
/// which is a low-level, unmanaged socket family. The caller must ensure that the socket is
/// properly configured and used to avoid errors and security vulnerabilities.
///
/// The socket is optimized for sending and receiving small UDP packets in low-latency P2P networks.
/// To minimize overhead, it does not support packets larger than 2048 bytes and dynamic frame allocation.
///  So:
///      8MB is the size for the UMEM, which is 4096 frames of 2048 bytes each.
///      For Tx direction, it uses
///        all frames for outgoing packets.
///      For Rx direction, it uses
///        all frames for incoming packets.
///      For Both direction, it uses
///       2048 frames for outgoing packets and 2048 frames for incoming packets.
///
/// By default, zero-copy is enabled if the network interface supports it,
/// unless explicitly disabled in the configuration.
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
    let (mut tx_ring, c_ring) = if direction == Direction::Rx {
        (Ring::default(), Ring::default())
    } else {
        (
            RingType::Tx.mmap(raw_fd, &offsets, tx_ring_size)?,
            RingType::Completion.mmap(raw_fd, &offsets, tx_ring_size)?,
        )
    };

    // Mapping Rx rings in case of Rx and Both direction
    let (rx_ring, f_ring) = if direction == Direction::Tx {
        (Ring::default(), Ring::default())
    } else {
        (
            RingType::Rx.mmap(raw_fd, &offsets, rx_ring_size)?,
            RingType::Fill.mmap(raw_fd, &offsets, rx_ring_size)?,
        )
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

    let inner = Arc::new(Inner { umem, fd });

    let tx_socket = if direction != Direction::Rx {
        Some(TxSocket::new(Some(inner.clone()), tx_ring, c_ring, 0))
    } else {
        None
    };

    let rx_socket = if direction != Direction::Tx {
        Some(RxSocket::new(
            Some(inner.clone()),
            rx_ring,
            f_ring,
            tx_ring_size,
        ))
    } else {
        None
    };

    Ok((tx_socket, rx_socket))
}

/// Creates a `TxSocket` that can be used for sending packets.
///
/// # Parameters
///
/// - `if_index`: The index of the network interface to use.
/// - `if_queue`: The queue ID of the network interface to use.
/// - `config`: An optional `XdpConfig` that can be used to customize the socket.
///
/// # Return value
///
/// Returns a `Result` containing a `TxSocket` if the socket was created successfully,
/// or an `io::Error` if an error occurred.
///
/// # Errors
///
/// Returns an `io::Error` if the socket could not be created.
pub fn create_tx_socket(
    if_index: u32,
    if_queue: u32,
    config: Option<XdpConfig>,
) -> Result<TxSocket, io::Error> {
    let (tx_socket, _) = create_socket(if_index, if_queue, Direction::Tx, config)?;
    Ok(tx_socket
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to create Tx socket"))?)
}

pub fn create_rx_socket(
    if_index: u32,
    if_queue: u32,
    config: Option<XdpConfig>,
) -> Result<RxSocket, io::Error> {
    let (_, rx_socket) = create_socket(if_index, if_queue, Direction::Rx, config)?;
    Ok(rx_socket
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to create Rx socket"))?)
}

pub fn create_bi_socket(
    if_index: u32,
    if_queue: u32,
    config: Option<XdpConfig>,
) -> Result<(TxSocket, RxSocket), io::Error> {
    let (tx_socket, rx_socket) = create_socket(if_index, if_queue, Direction::Rx, config)?;
    Ok((
        tx_socket
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to create Tx socket"))?,
        rx_socket
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to create Rx socket"))?,
    ))
}

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

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(i32)]
pub enum Direction {
    Tx = 0,
    Rx = 1,
    Both = -1,
}

#[derive(Debug, Copy, Clone, Default)]
pub struct XdpConfig {
    // if None Kernel is used XDP_ZEROCOPY if this ability is available
    // you can set if to enforce behaviour
    pub zero_copy: Option<bool>,
    // if None and HugePages are available, they will be used
    pub huge_page: Option<bool>,
    // if None or true then XDP_USE_NEED_WAKEUP is used in socket binding
    pub need_wakeup: Option<bool>,
}
