use crate::mmap::OwnedMmap;
use std::sync::atomic::AtomicU32;
use std::{io, ptr, slice};

pub const FRAME_SIZE: usize = 2048;
pub const FRAME_COUNT: usize = 4096;

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
pub struct Ring<T> {
    pub mmap: RingMmap<T>,
    pub len: usize,
}

impl<T> Ring<T>
where
    T: Copy,
{
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
        })
    }
    pub fn consumer(&self) -> u32 {
        unsafe { (*self.mmap.consumer).load(std::sync::atomic::Ordering::Acquire) }
    }
    pub fn producer(&self) -> u32 {
        unsafe { (*self.mmap.producer).load(std::sync::atomic::Ordering::Acquire) }
    }
    pub fn update_producer(&mut self, value: u32) {
        unsafe {
            (*self.mmap.producer).store(value, std::sync::atomic::Ordering::Release);
        }
    }
    pub fn update_consumer(&mut self, value: u32) {
        unsafe {
            (*self.mmap.consumer).store(value, std::sync::atomic::Ordering::Release);
        }
    }
    
    pub fn flags(&self) -> u32 {
        unsafe {
            (*self.mmap.flags).load(std::sync::atomic::Ordering::Acquire)
        }
    }
    pub fn increment(&self, value: &mut u32) -> u32 {
        *value = (*value + 1) & (self.len - 1) as u32;
        *value
    }

    pub fn mut_desc_at(&mut self, index: u32) -> &mut T {
        debug_assert!((index as usize) < self.len);
        unsafe { &mut *self.mmap.desc.add(index as usize) }
    }

    pub fn desc_at(&self, index: u32) -> T {
        debug_assert!((index as usize) < self.len);
        unsafe { *self.mmap.desc.add(index as usize) }
    }
}

impl Ring<XdpDesc> {
    pub fn fill(&mut self, start_frame: u32) {
        debug_assert!((start_frame as usize) < self.len);
        for i in 0..self.len as u32 {
            let desc = self.mut_desc_at(i + start_frame);
            *desc = XdpDesc {
                addr: i as u64 * FRAME_SIZE as u64,
                len: 0,
                options: 0,
            }
        }
    }
    pub fn mut_bytes_at(&mut self, ptr: *mut u8, index: u32, len: usize) -> &mut [u8] {
        debug_assert!(index < FRAME_COUNT as u32);
        debug_assert!((len as u32) < FRAME_SIZE as u32);
        let desc = self.mut_desc_at(index);
        debug_assert!(FRAME_SIZE*FRAME_COUNT > desc.addr as usize + len);
        unsafe {
            let ptr = ptr.offset(desc.addr as isize);
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

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum RingType {
    Tx,
    Rx,
    Fill,
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
