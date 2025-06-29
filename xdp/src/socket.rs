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

use crate::mmap::{OwnedMmap, Ring, XdpDesc};
use std::cmp::PartialEq;
use std::io;
use std::os::fd::{FromRawFd as _, OwnedFd};

/*
   This socket is optimized for sending and receiving small UDP packets in low-latency P2P networks.
   To minimize overhead, it does not support packets larger than 2048 bytes and dynamic frame allocation.
   So:
       8MB is the size for the UMEM, which is 4096 frames of 2048 bytes each.
       For Tx direction, it uses
         all frames for outgoing packets.
       For Rx direction, it uses
         all frames for incoming packets.
       For Both direction, it uses
        2048 frames for outgoing packets and 2048 frames for incoming packets.

   By default, zero-copy is enabled if the network interface supports it,
   unless explicitly disabled in the configuration.
*/
impl AfXdpSocket {
    pub fn new(
        device_queue: DeviceQueue,
        direction: Direction,
        config: Option<AfXdpConfig>,
    ) -> Result<Self, io::Error> {
        let frame_count = 4096usize; // Total frames for UMEM
        let frame_size = 2048usize; // Default frame size
        let (rx_ring_size, tx_ring_size) = match direction {
            Direction::Tx => (0, frame_count), // all frames for outgoing packets
            Direction::Rx => (frame_count, 0), // all frames for incoming packets
            Direction::Both => (frame_count / 2, frame_count / 2), // split frames for both directions
        };
        let huge_page = config.and_then(|cfg| cfg.no_huge_page).unwrap_or(true);
        let page_size = if !huge_page {
            unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
        } else {
            2 * 1024 * 1024 // 2MB huge page size
        };
        let aligned_size = (frame_count * frame_size + page_size - 1) & !(page_size - 1);
        let bpf_features = unsafe {
            let mut opts: libbpf_sys::bpf_xdp_query_opts = std::mem::zeroed();
            if libbpf_sys::bpf_xdp_query(
                device_queue.if_index as libc::c_int,
                libbpf_sys::XDP_FLAGS_DRV_MODE as libc::c_int,
                &mut opts,
            ) < 0
            {
                return Err(io::Error::last_os_error());
            }
            opts.feature_flags as u32
        };

        if direction != Direction::Tx && (bpf_features & 2/*NETDEV_XDP_ACT_REDIRECT*/ == 0) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Device does not support XDP redirection",
            ));
        }

        let (fd, raw_fd) = unsafe {
            let fd = libc::socket(libc::AF_XDP, libc::SOCK_RAW | libc::SOCK_CLOEXEC, 0);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            (OwnedFd::from_raw_fd(fd), fd)
        };

        let umem = OwnedMmap::mmap(aligned_size, huge_page).map_err(|e| {
            log::error!("Failed to allocate UMEM: {}", e);
            e
        })?;

        let reg = unsafe {
            libc::xdp_umem_reg {
                addr: umem.as_void_ptr() as u64,
                len: umem.len() as u64,
                chunk_size: frame_size as u32,
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
                return Err(io::Error::last_os_error());
            }
        }

        let set_ring_size = |ring, ring_size: usize| unsafe {
            if libc::setsockopt(
                raw_fd,
                libc::SOL_XDP,
                ring,
                &ring_size as *const _ as *const libc::c_void,
                size_of::<u32>() as libc::socklen_t,
            ) < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        };

        set_ring_size(libc::XDP_UMEM_COMPLETION_RING, tx_ring_size)?;
        set_ring_size(libc::XDP_TX_RING, tx_ring_size)?;
        set_ring_size(libc::XDP_UMEM_FILL_RING, rx_ring_size)?;
        set_ring_size(libc::XDP_RX_RING, rx_ring_size)?;

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

        let (completion_ring, tx_ring) = if direction == Direction::Rx {
            (Ring::default(), Ring::default())
        } else {
            let ring = Ring::mmap(
                raw_fd,
                tx_ring_size,
                libc::XDP_UMEM_PGOFF_COMPLETION_RING,
                &offsets.cr,
            )?;
            ring.fill(0);
            (
                ring,
                Ring::mmap(
                    raw_fd,
                    tx_ring_size,
                    libc::XDP_PGOFF_TX_RING as u64,
                    &offsets.tx,
                )?,
            )
        };

        let (fill_ring, rx_ring) = if direction == Direction::Tx {
            (Ring::default(), Ring::default())
        } else {
            let ring = Ring::mmap(
                raw_fd,
                rx_ring_size,
                libc::XDP_UMEM_PGOFF_FILL_RING,
                &offsets.fr,
            )?;
            ring.fill(tx_ring_size as u64);
            (
                ring,
                Ring::mmap(
                    raw_fd,
                    rx_ring_size,
                    libc::XDP_PGOFF_RX_RING as u64,
                    &offsets.rx,
                )?,
            )
        };

        let zero_copy = match bpf_features & 8/*NETDEV_XDP_ACT_XSK_ZEROCOPY*/ != 0 {
            true if !config.and_then(|cfg| cfg.no_zero_copy).unwrap_or(false) => libc::XDP_ZEROCOPY,
            _ => libc::XDP_COPY,
        };

        let sxdp = libc::sockaddr_xdp {
            sxdp_family: libc::AF_XDP as libc::sa_family_t,
            sxdp_flags: libc::XDP_USE_NEED_WAKEUP | zero_copy,
            sxdp_ifindex: device_queue.if_index,
            sxdp_queue_id: device_queue.queue_id.0 as u32,
            sxdp_shared_umem_fd: 0,
        };

        if unsafe {
            libc::bind(
                raw_fd,
                &sxdp as *const _ as *const libc::sockaddr,
                size_of::<libc::sockaddr_xdp>() as libc::socklen_t,
            ) < 0
        } {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            fd,
            umem,
            direction,
            tx_ring,
            completion_ring,
            rx_ring,
            fill_ring,
        })
    }
}

pub struct QueueId(pub u8);
pub struct DeviceQueue {
    pub if_index: u32,
    pub queue_id: QueueId,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Direction {
    Tx,
    Rx,
    Both,
}

pub struct AfXdpSocket {
    pub fd: OwnedFd,
    pub umem: OwnedMmap,
    pub direction: Direction,
    pub tx_ring: Ring<XdpDesc>,
    pub completion_ring: Ring<u64>,
    pub rx_ring: Ring<XdpDesc>,
    pub fill_ring: Ring<u64>,
}

#[derive(Debug, Copy, Clone, Default)]
pub struct AfXdpConfig {
    pub no_zero_copy: Option<bool>,
    pub no_huge_page: Option<bool>,
}
