use crate::mmap::{OwnedMmap, Ring, XdpDesc, mmap_ring};
use std::cmp::PartialEq;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
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
        let frame_count = 4096usize; // Total frames for UMEM
        let frame_size = 2048usize; // Default frame size
        let (rx_ring_size, tx_ring_size) = match direction {
            Direction::Tx => (0, frame_count), // all frames for outgoing packets
            Direction::Rx => (frame_count, 0), // all frames for incoming packets
            Direction::Both => (frame_count / 2, frame_count / 2), // half frames for incoming packets
        }; // none for incoming packets
        let tx_ring_size = frame_count - rx_ring_size; // rest frames for outgoing packets
        let frame_count = tx_ring_size + rx_ring_size; // Default frame size
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

        let fd = unsafe {
            let fd = libc::socket(libc::AF_XDP, libc::SOCK_RAW | libc::SOCK_CLOEXEC, 0);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            OwnedFd::from_raw_fd(fd)
        };

        let umem = unsafe {
            let ptr = libc::mmap(
                ptr::null_mut(),
                aligned_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE
                    | libc::MAP_ANONYMOUS
                    | if huge_page { libc::MAP_HUGETLB } else { 0 },
                -1,
                0,
            );
            if ptr == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            OwnedMmap(ptr, aligned_size)
        };

        let reg = libc::xdp_umem_reg {
            addr: umem.as_void_ptr() as u64,
            len: umem.len() as u64,
            chunk_size: frame_size as u32,
            headroom: 0,
            flags: 0,
            tx_metadata_len: 0,
        };

        unsafe {
            if libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_XDP,
                libc::XDP_UMEM_REG,
                &reg as *const _ as *const libc::c_void,
                size_of::<libc::xdp_umem_reg>() as libc::socklen_t,
            ) < 0
            {
                return Err(io::Error::last_os_error());
            }

            for ring in [libc::XDP_UMEM_COMPLETION_RING, libc::XDP_TX_RING] {
                if libc::setsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_XDP,
                    ring,
                    &tx_ring_size as *const _ as *const libc::c_void,
                    size_of::<u32>() as libc::socklen_t,
                ) < 0
                {
                    return Err(io::Error::last_os_error());
                }
            }

            for ring in [libc::XDP_UMEM_FILL_RING, libc::XDP_RX_RING] {
                if libc::setsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_XDP,
                    ring,
                    &rx_ring_size as *const _ as *const libc::c_void,
                    size_of::<u32>() as libc::socklen_t,
                ) < 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
        }

        let mut offsets: libc::xdp_mmap_offsets = unsafe { std::mem::zeroed() };
        let mut optlen = size_of::<libc::xdp_mmap_offsets>() as libc::socklen_t;

        unsafe {
            if libc::getsockopt(
                fd.as_raw_fd(),
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
            (Ring::<u64>::default(), Ring::<XdpDesc>::default())
        } else {
            let ring = mmap_ring(
                fd.as_raw_fd(),
                tx_ring_size,
                libc::XDP_UMEM_PGOFF_COMPLETION_RING,
                &offsets.cr,
            )?;
            ring.fill(0);
            (
                ring,
                mmap_ring(
                    fd.as_raw_fd(),
                    tx_ring_size,
                    libc::XDP_PGOFF_TX_RING as u64,
                    &offsets.tx,
                )?,
            )
        };

        let (fill_ring, rx_ring) = if direction == Direction::Tx {
            (Ring::<u64>::default(), Ring::<XdpDesc>::default())
        } else {
            let ring = mmap_ring(
                fd.as_raw_fd(),
                rx_ring_size,
                libc::XDP_UMEM_PGOFF_FILL_RING,
                &offsets.fr,
            )?;
            ring.fill(tx_ring_size as u64);
            (
                ring,
                mmap_ring(
                    fd.as_raw_fd(),
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
                fd.as_raw_fd(),
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
