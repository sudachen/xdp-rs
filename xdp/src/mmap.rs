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
use std::{io, ptr, slice};

pub const FRAME_SIZE: usize = 2048;
pub const FRAME_COUNT: usize = 4096;

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

pub struct RingMmap {
    pub mmap: OwnedMmap,
    pub producer: *mut AtomicU32,
    pub consumer: *mut AtomicU32,
    pub desc: *mut XdpDesc,
    pub flags: *mut AtomicU32,
}
impl Default for RingMmap {
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
#[derive(Debug, Clone, Copy, Default)]
pub struct XdpDesc {
    pub addr: u64,
    pub len: u32,
    pub options: u32,
}

impl XdpDesc {
    pub fn new(addr: u64, len: u32, options: u32) -> Self {
        XdpDesc { addr, len, options }
    }
}

#[derive(Default)]
pub struct Ring {
    pub mmap: RingMmap,
    pub len: usize,
}

impl Ring {
    pub fn mmap(
        fd: i32,
        len: usize,
        ring_type: u64,
        offsets: &libc::xdp_ring_offset,
    ) -> Result<Self, io::Error> {
        debug_assert!(len.is_power_of_two());
        Ok(Ring{
            mmap: mmap_ring(fd, len * size_of::<XdpDesc>(), offsets, ring_type)?,
            len
        })
    }
    pub fn consumer(&self) -> u32 {
        unsafe { (*self.mmap.consumer).load(std::sync::atomic::Ordering::Acquire) }
    }
    pub fn producer(&self) -> u32 {
        unsafe { (*self.mmap.consumer).load(std::sync::atomic::Ordering::Acquire) }
    }
    pub fn update_producer(&mut self, value: u32) {
        unsafe {
            (*self.mmap.producer).store(value, std::sync::atomic::Ordering::SeqCst);
        }
    }
    pub fn update_consumer(&mut self, value: u32) {
        unsafe {
            (*self.mmap.consumer).store(value, std::sync::atomic::Ordering::SeqCst);
        }
    }
    pub fn increment(&self, value: &mut u32) -> u32 {
        *value = (*value + 1) & (FRAME_COUNT -1) as u32;
        *value
    }

    pub fn mut_desc_at(&mut self, index: u32) -> &mut XdpDesc {
        debug_assert!((index as usize) < self.len);
        unsafe { &mut *self.mmap.desc.add(index as usize) }
    }

    pub fn desc_at(&self, index: u32) -> XdpDesc {
        debug_assert!((index as usize) < self.len);
        unsafe { *self.mmap.desc.add(index as usize) }
    }

    pub fn fill(&mut self, start_frame: u32) {
        debug_assert!((start_frame as usize) < self.len);
        for i in 0 ..self.len as u32 {
            let desc = self.mut_desc_at(i + start_frame);
            *desc = XdpDesc {
                addr: i as u64 * FRAME_SIZE as u64,
                len: 0,
                options: 0
            }
        }
    }
    pub fn mut_bytes_at(&mut self, umem: &mut OwnedMmap, index: u32, len: usize) -> &mut [u8] {
        debug_assert!(index < FRAME_COUNT as u32);
        debug_assert!((len as u32) < FRAME_SIZE as u32);
        let desc = self.mut_desc_at(index);
        debug_assert!(umem.1 > desc.addr as usize + len);
        unsafe {
            let ptr = umem.as_u8_ptr().offset(desc.addr as isize);
            desc.len = len as u32;
            slice::from_raw_parts_mut(ptr, len)
        }
    }

    pub fn set(&mut self, index: u32, len: u32) {
        let desc = self.mut_desc_at(index);
        *desc = XdpDesc {
            addr: (index as u64 * FRAME_SIZE as u64),
            len,
            options: 0,
        };
    }
}

pub fn mmap_ring(
    fd: i32,
    size: usize,
    offsets: &libc::xdp_ring_offset,
    ring_type: u64,
) -> Result<RingMmap, io::Error> {
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
    let desc = unsafe { map_addr.add(offsets.desc as usize) as *mut XdpDesc };
    let flags = unsafe { map_addr.add(offsets.flags as usize) as *mut AtomicU32 };
    Ok(RingMmap {
        mmap: OwnedMmap(map_addr, map_size),
        producer,
        consumer,
        desc,
        flags,
    })
}
