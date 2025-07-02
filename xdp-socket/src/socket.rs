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
use crate::ring::{FRAME_COUNT, FRAME_SIZE, Ring, RingType, XdpDesc};
use std::cmp::PartialEq;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::sync::atomic::Ordering;
use std::{io, ptr};
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

        // Setting rings sizes
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
            return Err(io::Error::other(format!(
                "Failed to bind: {}",
                io::Error::last_os_error()
            )));
        }

        tx_ring.fill(0);

        Ok(Self {
            fd,
            umem,
            direction,
            tx_ring,
            c_ring,
            rx_ring,
            f_ring,
            rx_head: 0,
            tx_tail: tx_ring_size.saturating_sub(1) as u32,
        })
    }

    pub fn wakeup(&self, enforce: bool, _direction: Direction) -> Result<(), io::Error> {
        let need_wakeup = enforce
            || unsafe {
                (*self.tx_ring.mmap.flags).load(Ordering::Relaxed) & libc::XDP_RING_NEED_WAKEUP != 0
            };
        if need_wakeup
            && 0 > unsafe {
                libc::sendto(
                    self.fd.as_raw_fd(),
                    ptr::null(),
                    0,
                    libc::MSG_DONTWAIT,
                    ptr::null(),
                    0,
                )
            }
        {
            match io::Error::last_os_error().raw_os_error() {
                None | Some(libc::EBUSY | libc::ENOBUFS | libc::EAGAIN) => {}
                Some(libc::ENETDOWN) => {
                    // TODO: better handling
                    log::warn!("network interface is down, cannot wake up");
                }
                Some(e) => {
                    return Err(io::Error::from_raw_os_error(e));
                }
            }
        }
        Ok(())
    }
}

//const NETDEV_XDP_ACT_REDIRECT: u32 = 2;
pub fn xdp_features(if_index: u32) -> io::Result<u32> {
    Ok(unsafe {
        let mut opts: libbpf_sys::bpf_xdp_query_opts = std::mem::zeroed();
        opts.sz = size_of::<libbpf_sys::bpf_xdp_query_opts>() as u64;
        if libbpf_sys::bpf_xdp_query(
            if_index as libc::c_int,
            libbpf_sys::XDP_FLAGS_DRV_MODE as libc::c_int,
            &mut opts,
        ) < 0
        {
            return Err(io::Error::other(format!(
                "Failed to query XDP features: {}",
                io::Error::last_os_error()
            )));
        }
        opts.feature_flags as u32
    })
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

pub fn setup_umem(raw_fd: libc::c_int, config: Option<&AfXdpConfig>) -> io::Result<OwnedMmap> {
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

pub struct QueueId(pub u8);
pub struct DeviceQueue {
    pub if_index: u32,
    pub queue_id: QueueId,
}

impl DeviceQueue {
    pub fn form_ifindex(if_index: u32) -> Self {
        Self {
            if_index,
            queue_id: QueueId(0),
        }
    }

    pub fn form_ifindex_and_queue(if_index: u32, queue_id: u8) -> Self {
        Self {
            if_index,
            queue_id: QueueId(queue_id),
        }
    }
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
    pub c_ring: Ring<u64>,
    pub rx_ring: Ring<XdpDesc>,
    pub f_ring: Ring<u64>,
    pub rx_head: u32,
    pub tx_tail: u32,
}

#[derive(Debug, Copy, Clone, Default)]
pub struct AfXdpConfig {
    // if None Kernel is used XDP_ZEROCOPY if this ability is available
    // you can set if to enforce behaviour
    pub zero_copy: Option<bool>,
    // if None and HugePages are available, they will be used
    pub huge_page: Option<bool>,
    // if None or true then XDP_USE_NEED_WAKEUP is used in socket binding
    pub need_wakeup: Option<bool>,
}
