//
// mmap.rs - Memory Mapping for AF_XDP Rings and UMEM
//
// Purpose:
//   This module provides safe and ergonomic abstractions for memory mapping (mmap) of AF_XDP
//   ring buffers and UMEM regions. It is essential for enabling zero-copy packet I/O between
//   user space and the kernel.
//
// How it works:
//   - Wraps low-level mmap operations for XDP rings (Rx, Tx, Fill, Comp) and UMEM.
//   - Provides safe Rust types for managing mapped memory and ring access.
//   - Handles offset calculations, alignment, and size checks for AF_XDP ring layouts.
//
// Main components:
//   - RingMmap, OwnedMmap: Safe wrappers for memory-mapped regions.
//   - Ring<T>: Abstraction for XDP ring buffer access and management.
//   - mmap_ring, mmap_ring_at: Functions for mapping rings with correct offsets and permissions.
//   - Utility helpers for pointer arithmetic, offset handling, and ring setup.
//

use std::sync::atomic::AtomicU32;
use std::{io, ptr};

pub struct OwnedMmap(pub *mut libc::c_void, pub usize);

impl OwnedMmap {
    pub fn new(ptr: *mut libc::c_void, size: usize) -> Self {
        OwnedMmap(ptr, size)
    }
    pub fn mmap(aligned_size: usize, huge_page: bool) -> Result<Self, io::Error> {
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                aligned_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE
                    | libc::MAP_ANONYMOUS
                    | if huge_page { libc::MAP_HUGETLB } else { 0 },
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(OwnedMmap(ptr, aligned_size))
    }
    pub fn as_void_ptr(&self) -> *mut libc::c_void {
        self.0
    }
    pub fn as_u8_ptr(&mut self) -> *mut u8 {
        self.0 as *mut u8
    }
    pub fn len(&self) -> usize {
        self.1
    }
    pub fn is_empty(&self) -> bool {
        self.1 == 0
    }
}

impl Drop for OwnedMmap {
    fn drop(&mut self) {
        unsafe {
            if self.0 != libc::MAP_FAILED {
                let res = libc::munmap(self.0, self.1);
                if res < 0 {
                    log::error!("Failed to unmap memory: {}", io::Error::last_os_error());
                }
            }
        }
    }
}

pub struct RingMmap<T> {
    pub mmap: OwnedMmap,
    pub producer: *mut AtomicU32,
    pub consumer: *mut AtomicU32,
    pub desc: *mut T,
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

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct XdpDesc {
    pub addr: u64,
    pub len: u32,
    pub options: u32,
}

#[derive(Default)]
pub struct Ring<T> {
    pub mmap: RingMmap<T>,
    pub size: usize,
}

impl<T> Ring<T> {
    pub fn mmap(
        fd: i32,
        size: usize,
        ring_type: u64,
        offsets: &libc::xdp_ring_offset,
    ) -> Result<Self, io::Error> {
        Ok(Ring::<T> {
            mmap: mmap_ring(fd, size * size_of::<T>(), offsets, ring_type)?,
            size,
        })
    }
}

impl Ring<u64> {
    pub fn fill(&self, start_frame: u64) {
        unsafe {
            for i in 0..self.size {
                let desc = self.mmap.desc.add(i);
                *desc = start_frame + i as u64;
            }
        }
    }
}

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
