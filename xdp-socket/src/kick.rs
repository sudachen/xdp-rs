//! # XDP Socket Kernel Wakeup
//!
//! ## Purpose
//!
//! This file implements the `kick` method for the `Socket`. The purpose of this
//! method is to notify the kernel to process packets in XDP rings, especially
//! when `XDP_USE_NEED_WAKEUP` is in use.
//!
//! ## How it works
//!
//! The `kick` method checks the `XDP_RING_NEED_WAKEUP` flag in the ring's flags
//! field. If set, it performs a zero-length `sendto` syscall to signal the kernel.
//! This prompts the kernel to check the rings for new descriptors to process.
//!
//! ## Main components
//!
//! - `kick`: Main method to trigger kernel wakeup for XDP socket rings.

#![allow(private_interfaces)]
#![allow(private_bounds)]

use std::sync::atomic::Ordering;
use std::{io, ptr};

use crate::socket::{_Direction, Commit_, RingError, Socket};

/// Implements the kernel wakeup logic for `Socket`.
impl<const T: _Direction> Socket<T>
where
    Socket<T>: Commit_<T>,
{
    /// Wakes up the kernel to process descriptors in the rings.
    ///
    /// This method is used to notify the kernel that it needs to process packets,
    /// which is particularly important when the `XDP_USE_NEED_WAKEUP` flag is set
    /// on the socket. It checks if the `XDP_RING_NEED_WAKEUP` flag is set in the
    /// ring's flags field and, if so, performs a syscall to wake up the kernel.
    ///
    /// # How it works
    ///
    /// It performs a `sendto` syscall with a zero-length buffer. This syscall does not transfer
    /// any data but acts as a signal to the kernel.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success. On failure, it returns an `io::Error`, except
    /// for certain non-critical errors like `EBUSY` or `EAGAIN`. A warning is
    /// logged for `ENETDOWN`.
    pub fn kick(&self) -> Result<(), io::Error> {
        let need_wakeup = unsafe {
            (*self.x_ring.mmap.flags).load(Ordering::Relaxed) & libc::XDP_RING_NEED_WAKEUP != 0
        };

        if need_wakeup {
            let ret = unsafe {
                libc::sendto(
                    self.raw_fd,
                    ptr::null(),
                    0,
                    libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
                    ptr::null(),
                    0,
                )
            };

            if ret < 0 {
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
        }
        Ok(())
    }

    /// Commits a number of descriptors and notifies the kernel to process them.
    ///
    /// This method first calls `commit_` to commit `n` descriptors, and then
    /// calls `kick` to notify the kernel to process the descriptors.  The
    /// `commit_` method is used to finalize operations on descriptors and make
    /// them available to the kernel, and the `kick` method is used to signal to
    /// the kernel that it needs to process the descriptors.
    ///
    /// # Returns
    ///
    /// This method returns `Ok(())` on success.  If `commit_` fails, it returns
    /// a `RingError`.  If `kick` fails, it maps the error to a `RingError` using
    /// `RingError::Io`.
    pub fn commit_and_kick(&mut self, n: usize) -> Result<(), RingError> {
        self.commit_(n)?;
        self.kick().map_err(RingError::Io)
    }
}
