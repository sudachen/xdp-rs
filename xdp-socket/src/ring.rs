//! # AF_XDP Ring Buffer Management
//!
//! ## Purpose
//!
//! This file defines the core data structures and logic for managing the various
//! ring buffers used in AF_XDP sockets. These rings are the primary mechanism for
//! communication between the userspace application and the kernel.
//!
//! ## How it works
//!
//! It provides a generic `Ring<T>` struct that encapsulates a memory-mapped ring buffer.
//! This struct provides methods for atomically accessing and updating the producer and
//! consumer indices, accessing descriptors, and checking ring flags. It also defines
//! the `XdpDesc` struct for packet descriptors. The `RingType` enum helps manage the
//! specifics of the four different rings (TX, RX, Fill, Completion), such as their
//! memory map offsets and socket option names.
//!
//! ## Main components
//!
//! - `Ring<T>`: A generic struct representing a shared memory ring buffer.
//! - `RingMmap<T>`: A struct holding the raw memory-mapped components of a ring.
//! - `XdpDesc`: The descriptor structure for packets in the TX and RX rings, containing
//!   address, length, and options.
//! - `RingType`: An enum to differentiate between ring types and handle their specific
//!   setup requirements.

use crate::mmap::OwnedMmap;
use std::sync::atomic::AtomicU32;
use std::{io, mem::size_of, ptr, slice};

/// The size of a single frame in the UMEM, typically 2KB or 4KB.
pub const FRAME_SIZE: usize = 2048;
/// The default number of frames to allocate for the UMEM.
pub const FRAME_COUNT: usize = 4096;

/// Holds the raw memory-mapped components of a ring buffer.
///
/// This struct contains raw pointers to the producer/consumer indices, the descriptor
/// array, and flags within the memory-mapped region. It is managed by the `Ring` struct.
pub struct RingMmap<T> {
    /// The memory-mapped region owned by this struct.
    pub mmap: OwnedMmap,
    /// A pointer to the atomic producer index of the ring.
    pub producer: *mut AtomicU32,
    /// A pointer to the atomic consumer index of the ring.
    pub consumer: *mut AtomicU32,
    /// A pointer to the beginning of the descriptor array.
    pub desc: *mut T,
    /// A pointer to the atomic flags field of the ring.
    pub flags: *mut AtomicU32,
}
impl<T> Default for RingMmap<T> {
    fn default() -> Self {
        RingMmap {
            mmap: OwnedMmap(ptr::null_mut(), 0),
            producer: ptr::null_mut(),
            consumer: ptr::null_mut(),
            desc: ptr::null_mut(),
            flags: ptr::null_mut(),
        }
    }
}

/// An XDP descriptor, used in the TX and RX rings.
///
/// This struct corresponds to `struct xdp_desc` in the kernel and describes a
/// single packet buffer in the UMEM.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct XdpDesc {
    /// The address of the packet data within the UMEM.
    pub addr: u64,
    /// The length of the packet data.
    pub len: u32,
    /// Options for the descriptor, currently unused.
    pub options: u32,
}

impl XdpDesc {
    /// Creates a new `XdpDesc`.
    pub fn new(addr: u64, len: u32, options: u32) -> Self {
        XdpDesc { addr, len, options }
    }
}

/// A generic, safe wrapper for an AF_XDP ring buffer.
///
/// This struct provides safe methods to interact with a memory-mapped ring,
/// handling atomic operations for producer/consumer indices and access to descriptors.
#[derive(Default)]
pub struct Ring<T> {
    /// The memory-mapped components of the ring.
    pub mmap: RingMmap<T>,
    /// The number of descriptors the ring can hold.
    pub len: usize,
    /// A mask used for wrapping around the ring (len - 1).
    pub mod_mask: u32,
}

impl<T> Ring<T>
where
    T: Copy,
{
    /// Returns the size of a single UMEM frame.
    #[inline]
    pub fn frame_size(&self) -> u64 {
        FRAME_SIZE as u64
    }

    /// Memory-maps a ring from a file descriptor.
    ///
    /// # How it works
    ///
    /// This function calls the lower-level `mmap_ring` function to perform the `mmap`
    /// syscall with the correct offsets for the given ring type. It then initializes
    /// a `Ring` struct to manage the mapped memory.
    pub fn mmap(
        fd: i32,
        len: usize,
        ring_type: u64,
        offsets: &libc::xdp_ring_offset,
    ) -> Result<Self, io::Error> {
        debug_assert!(len.is_power_of_two());
        Ok(Ring {
            mmap: mmap_ring(fd, len * size_of::<T>(), offsets, ring_type)?,
            len,
            mod_mask: len as u32 - 1,
        })
    }
    /// Atomically reads the consumer index of the ring.
    pub fn consumer(&self) -> u32 {
        unsafe { (*self.mmap.consumer).load(std::sync::atomic::Ordering::Acquire) }
    }
    /// Atomically reads the producer index of the ring.
    pub fn producer(&self) -> u32 {
        unsafe { (*self.mmap.producer).load(std::sync::atomic::Ordering::Acquire) }
    }
    /// Atomically updates the producer index of the ring.
    pub fn update_producer(&mut self, value: u32) {
        unsafe {
            (*self.mmap.producer).store(value, std::sync::atomic::Ordering::Release);
        }
    }
    /// Atomically updates the consumer index of the ring.
    pub fn update_consumer(&mut self, value: u32) {
        unsafe {
            (*self.mmap.consumer).store(value, std::sync::atomic::Ordering::Release);
        }
    }

    /// Atomically reads the flags of the ring.
    ///
    /// Flags can indicate states like `XDP_RING_NEED_WAKEUP`.
    pub fn flags(&self) -> u32 {
        unsafe {
            (*self.mmap.flags).load(std::sync::atomic::Ordering::Acquire)
        }
    }
    /// Increments a value, wrapping it around the ring size.
    pub fn increment(&self, value: &mut u32) -> u32 {
        *value = (*value + 1) & (self.len - 1) as u32;
        *value
    }

    /// Returns a mutable reference to the descriptor at a given index.
    ///
    /// # Panics
    ///
    /// This function will panic in debug builds if the index is out of bounds.
    pub fn mut_desc_at(&mut self, index: u32) -> &mut T {
        debug_assert!((index as usize) < self.len);
        unsafe { &mut *self.mmap.desc.add(index as usize) }
    }

    /// Returns a copy of the descriptor at a given index.
    ///
    /// # Panics
    ///
    /// This function will panic in debug builds if the index is out of bounds.
    pub fn desc_at(&self, index: u32) -> T {
        debug_assert!((index as usize) < self.len);
        unsafe { *self.mmap.desc.add(index as usize) }
    }
}

impl Ring<u64> {
    /// Fills the ring (typically the Fill Ring) with UMEM frame addresses.
    ///
    /// # Arguments
    /// * `start_frame` - The starting frame number to begin filling from.
    pub fn fill(&mut self, start_frame: u32) {
        for i in 0..self.len as u32 {
            let desc = self.mut_desc_at(i);
            *desc = (i + start_frame) as u64 * FRAME_SIZE as u64;
        }
    }
}

impl Ring<XdpDesc> {
    /// Fills the ring (typically the TX ring) with default `XdpDesc` values.
    ///
    /// This pre-populates the ring with descriptors pointing to corresponding
    /// UMEM frames.
    ///
    /// # Arguments
    /// * `start_frame` - The starting frame number to begin filling from.
    pub fn fill(&mut self, start_frame: u32) {
        for i in 0..self.len as u32 {
            let desc = self.mut_desc_at(i);
            *desc = XdpDesc {
                addr: (i + start_frame) as u64 * FRAME_SIZE as u64,
                len: 0,
                options: 0,
            }
        }
    }
    /// Returns a mutable byte slice for a packet buffer in the UMEM.
    ///
    /// This function gets the descriptor at `index`, calculates the memory address
    /// within the UMEM, and returns a mutable slice of `len` bytes. It also updates
    /// the descriptor's length field.
    ///
    /// # Panics
    ///
    /// This function will panic in debug builds if the index or length are out of bounds.
    pub fn mut_bytes_at(&mut self, ptr: *mut u8, index: u32, len: usize) -> &mut [u8] {
        #[cfg(not(feature="no_safety_checks"))]
        assert!(index < FRAME_COUNT as u32);
        #[cfg(not(feature="no_safety_checks"))]
        assert!((len as u32) < FRAME_SIZE as u32);

        let desc = self.mut_desc_at(index);

        #[cfg(not(feature="no_safety_checks"))]
        assert!(FRAME_SIZE * FRAME_COUNT > desc.addr as usize + len);

        unsafe {
            let ptr = ptr.offset(desc.addr as isize);
            desc.len = len as u32;
            slice::from_raw_parts_mut(ptr, len)
        }
    }

    /// Sets the descriptor at `index` to a specific length.
    ///
    /// The address is calculated based on the index and frame size.
    pub fn set(&mut self, index: u32, len: u32) {
        #[cfg(not(feature="no_safety_checks"))]
        assert!(index < FRAME_COUNT as u32);
        #[cfg(not(feature="no_safety_checks"))]
        assert!((len as u32) < FRAME_SIZE as u32);

        let desc = self.mut_desc_at(index);
        *desc = XdpDesc {
            addr: (index as u64 * FRAME_SIZE as u64),
            len,
            options: 0,
        };
    }
}

/// A low-level function to memory-map a single AF_XDP ring.
///
/// # How it works
///
/// It calculates the total size required for the mapping, including the area for
/// producer/consumer indices and the descriptor array. It then calls `libc::mmap`
/// with the appropriate file descriptor, size, and page offset for the given ring
/// type. On success, it returns a `RingMmap` containing pointers to the relevant
/// parts of the mapped region.
pub fn mmap_ring<T>(
    fd: i32,
    size: usize,
    offsets: &libc::xdp_ring_offset,
    ring_type: u64,
) -> Result<RingMmap<T>, io::Error> {
    let map_size = (offsets.desc as usize).saturating_add(size);
    let map_addr = unsafe {
        libc::mmap(
            ptr::null_mut(),
            map_size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED | libc::MAP_POPULATE,
            fd,
            ring_type as i64,
        )
    };
    if map_addr == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }
    let producer = unsafe { map_addr.add(offsets.producer as usize) as *mut AtomicU32 };
    let consumer = unsafe { map_addr.add(offsets.consumer as usize) as *mut AtomicU32 };
    let desc = unsafe { map_addr.add(offsets.desc as usize) as *mut T };
    let flags = unsafe { map_addr.add(offsets.flags as usize) as *mut AtomicU32 };
    Ok(RingMmap {
        mmap: OwnedMmap(map_addr, map_size),
        producer,
        consumer,
        desc,
        flags,
    })
}

/// An enum representing the four types of AF_XDP rings.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum RingType {
    /// The Transmit (TX) ring, for sending packets.
    Tx,
    /// The Receive (RX) ring, for receiving packets.
    Rx,
    /// The Fill ring, for providing the kernel with free UMEM frames.
    Fill,
    /// The Completion ring, for retrieving used UMEM frames from the kernel.
    Completion,
}

impl RingType {
    fn as_index(&self) -> libc::c_int {
        match self {
            RingType::Tx => libc::XDP_TX_RING,
            RingType::Rx => libc::XDP_RX_RING,
            RingType::Fill => libc::XDP_UMEM_FILL_RING,
            RingType::Completion => libc::XDP_UMEM_COMPLETION_RING,
        }
    }

    fn as_offset(&self) -> u64 {
        match self {
            RingType::Tx => libc::XDP_PGOFF_TX_RING as u64,
            RingType::Rx => libc::XDP_PGOFF_RX_RING as u64,
            RingType::Fill => libc::XDP_UMEM_PGOFF_FILL_RING,
            RingType::Completion => libc::XDP_UMEM_PGOFF_COMPLETION_RING,
        }
    }

    /// Sets the size of a specific ring via `setsockopt`.
    ///
    /// # Arguments
    /// * `raw_fd` - The raw file descriptor of the XDP socket.
    /// * `ring_size` - The number of descriptors for the ring.
    pub fn set_size(self, raw_fd: libc::c_int, mut ring_size: usize) -> io::Result<()> {
        if ring_size == 0 && (self == RingType::Fill || self == RingType::Completion) {
            ring_size = 1 // Fill and Completion rings must have at least one entry
        }
        unsafe {
            if libc::setsockopt(
                raw_fd,
                libc::SOL_XDP,
                self.as_index() as libc::c_int,
                &ring_size as *const _ as *const libc::c_void,
                size_of::<u32>() as libc::socklen_t,
            ) < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
    /// Memory-maps a ring of a specific type.
    ///
    /// This is a convenience method that selects the correct offsets from `xdp_mmap_offsets`
    /// based on the `RingType` and then calls the generic `Ring::mmap` function.
    ///
    /// # Arguments
    /// * `raw_fd` - The raw file descriptor of the XDP socket.
    /// * `offsets` - The struct containing the memory map offsets for all rings.
    /// * `ring_size` - The number of descriptors for the ring.
    pub fn mmap<T: Copy>(
        self,
        raw_fd: libc::c_int,
        offsets: &libc::xdp_mmap_offsets,
        ring_size: usize,
    ) -> io::Result<Ring<T>> {
        let ring_offs = match self {
            RingType::Tx => &offsets.tx,
            RingType::Rx => &offsets.rx,
            RingType::Fill => &offsets.fr,
            _ => &offsets.cr,
        };
        Ring::<T>::mmap(raw_fd, ring_size, self.as_offset(), ring_offs)
    }
}
