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

use std::fs::File;
use std::io::{BufRead as _, BufReader};
use std::{io, ptr};

pub struct OwnedMmap(pub *mut libc::c_void, pub usize);

impl OwnedMmap {
    pub fn new(ptr: *mut libc::c_void, size: usize) -> Self {
        OwnedMmap(ptr, size)
    }
    pub fn mmap(size: usize, huge_page: Option<bool>) -> Result<Self, io::Error> {
        // if not specified use huge pages, check if they are available
        let huge_tlb = if let Some(yes) = huge_page {
            yes
        } else {
            let info = get_hugepage_info()?;
            if let (Some(x), Some(2048)) = (info.free, info.size_kb) {
                x > 0
            } else {
                false
            }
        };
        let page_size = {
            if huge_tlb {
                2 * 1024 * 1024 // 2MB huge page size
            } else {
                unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
            }
        };
        let aligned_size = (size + page_size - 1) & !(page_size - 1);
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                aligned_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE
                    | libc::MAP_ANONYMOUS
                    | if huge_tlb {
                        libc::MAP_HUGETLB | libc::MAP_HUGE_2MB
                    } else {
                        0
                    },
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
            if self.0 != libc::MAP_FAILED && !self.0.is_null() {
                let res = libc::munmap(self.0, self.1);
                if res < 0 {
                    log::error!("Failed to unmap memory: {}", io::Error::last_os_error());
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct HugePageInfo {
    pub size_kb: Option<u64>,
    pub total: Option<u64>,
    pub free: Option<u64>,
}

pub fn get_hugepage_info() -> io::Result<HugePageInfo> {
    let file = File::open("/proc/meminfo")?;
    let reader = BufReader::new(file);
    let mut info = HugePageInfo::default();
    for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split(':').collect();

        if parts.len() == 2 {
            let key = parts[0].trim();
            let value_str = parts[1].trim().trim_end_matches(" kB");
            match key {
                "Hugepagesize" => info.size_kb = Some(value_str.parse().map_err(io::Error::other)?),
                "HugePages_Total" => {
                    info.total = Some(value_str.parse().map_err(io::Error::other)?)
                }
                "HugePages_Free" => info.free = Some(value_str.parse().map_err(io::Error::other)?),
                _ => {} // Ignore other lines
            }
        }
    }
    Ok(info)
}
