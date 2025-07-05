//! # Memory Mapping for UMEM
//!
//! ## Purpose
//!
//! This module provides safe abstractions for creating and managing memory-mapped (`mmap`)
//! regions, specifically for the AF_XDP UMEM (Userspace Memory). The UMEM is a critical
//! component for achieving zero-copy performance.
//!
//! ## How it works
//!
//! It defines an `OwnedMmap` struct that encapsulates a raw pointer to a memory-mapped
//! region and its size. This struct's implementation handles the low-level `libc::mmap`
//! call for allocation and `libc::munmap` in its `Drop` implementation to ensure the
//! memory is safely released. It also includes logic to check for and optionally use
//! huge pages to back the UMEM, which can improve performance by reducing TLB misses.
//!
//! ## Main components
//!
//! - `OwnedMmap`: A struct that acts as a safe owner of a memory-mapped region.
//! - `get_hugepage_info()`: A helper function that parses `/proc/meminfo` to determine if
//!   huge pages are available for use.

use std::fs::File;
use std::io::{BufRead as _, BufReader};
use std::{io, ptr};

/// A safe wrapper for a memory-mapped region.
///
/// This struct owns the memory-mapped pointer and ensures that `munmap` is called
/// when it goes out of scope, preventing memory leaks.
pub struct OwnedMmap(
    /// A raw pointer to the beginning of the memory-mapped area.
    pub *mut libc::c_void,
    /// The total size of the memory-mapped area in bytes.
    pub usize,
);

impl OwnedMmap {
    /// Constructs a new `OwnedMmap` from a raw pointer and size.
    ///
    /// This is a low-level constructor. Prefer `mmap` for new allocations.
    pub fn new(ptr: *mut libc::c_void, size: usize) -> Self {
        OwnedMmap(ptr, size)
    }

    /// Creates a new memory-mapped region.
    ///
    /// This function allocates a new anonymous, private memory-mapped region suitable
    /// for use as a UMEM. It can optionally be backed by huge pages.
    ///
    /// # How it works
    ///
    /// It first determines whether to use huge pages. If `huge_page` is `None`, it
    /// checks `/proc/meminfo` for available huge pages. It then calculates the
    /// required size aligned to the page size (standard or huge) and calls `libc::mmap`
    /// with the appropriate flags (`MAP_HUGETLB` if using huge pages).
    /// On success, it returns an `OwnedMmap` that manages the allocated memory.
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

    /// Returns the raw pointer to the memory-mapped region.
    pub fn as_void_ptr(&self) -> *mut libc::c_void {
        self.0
    }

    /// Returns a mutable raw pointer to the memory-mapped region as a byte slice.
    pub fn as_u8_ptr(&mut self) -> *mut u8 {
        self.0 as *mut u8
    }

    /// Returns the size of the memory-mapped region in bytes.
    pub fn len(&self) -> usize {
        self.1
    }

    /// Returns `true` if the memory-mapped region has a size of zero.
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

/// Contains information about the system's huge page configuration.
#[derive(Debug, Default)]
pub struct HugePageInfo {
    /// The size of a huge page in kilobytes.
    pub size_kb: Option<u64>,
    /// The total number of huge pages configured in the system.
    pub total: Option<u64>,
    /// The number of free (available) huge pages.
    pub free: Option<u64>,
}

/// Parses `/proc/meminfo` to get information about huge pages.
///
/// # How it works
///
/// It reads the `/proc/meminfo` pseudo-file line by line, looking for keys
/// `Hugepagesize`, `HugePages_Total`, and `HugePages_Free`. It parses their
/// corresponding values and returns them in a `HugePageInfo` struct.
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
