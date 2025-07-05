//! # XDP Socket Kernel Wakeup
//!
//! ## Purpose
//!
//! This file implements the `kick` method for the `Socket`. The purpose of this method
//! is to notify the kernel that it needs to process packets in one of the XDP rings,
//! especially when the `XDP_USE_NEED_WAKEUP` flag is in use.
//!
//! ## How it works
//!
//! The `kick` method checks if the `XDP_RING_NEED_WAKEUP` flag is set in the ring's
//! flags field. If it is (or if the wakeup is manually enforced), it performs a syscall
//! (`sendto` for TX, `recvfrom` for RX) with a zero-length buffer. This syscall does not
//! transfer data but serves as a signal to wake up the kernel and prompt it to check the
//! rings for new descriptors to process.
//!
//! ## Main components
//!
//! - `impl<const T:_Direction> Socket<T>`: An implementation block for the generic socket.
//! - `kick()`: The public method that performs the wakeup call to the kernel.

use std::{io, ptr};
use std::os::fd::AsRawFd;
use std::sync::atomic::Ordering;

use crate::socket::{_Direction, _RX, _TX, Socket};

/// Implements the kernel wakeup logic for `Socket`.
impl<const T: _Direction> Socket<T> {
    /// Wakes up the kernel to process descriptors in the rings.
    ///
    /// This method is used to notify the kernel that it needs to process packets,
    /// which is particularly important when the `XDP_USE_NEED_WAKEUP` flag is set
    /// on the socket. It checks if the `XDP_RING_NEED_WAKEUP` flag is set in the
    /// ring's flags field and, if so, performs a syscall to wake up the kernel.
    ///
    /// # How it works
    ///
    /// It performs a `sendto` (for TX) or `recvfrom` (for RX) syscall with a
    /// zero-length buffer. This syscall does not transfer any data but acts as a
    /// signal to the kernel.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success. On failure, it returns an `io::Error`, except
    /// for certain non-critical errors like `EBUSY` or `EAGAIN`. A warning is
    /// logged for `ENETDOWN`.
    pub fn kick(&self) -> Result<(), io::Error> {
        if let Some(inner) = &self.inner {
            let need_wakeup = unsafe {
                    (*self.x_ring.mmap.flags).load(Ordering::Relaxed) & libc::XDP_RING_NEED_WAKEUP
                        != 0
                };

            if need_wakeup {
                let ret = unsafe {
                    match T {
                        _TX => libc::sendto(
                            inner.fd.as_raw_fd(),
                            ptr::null(),
                            0,
                            libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
                            ptr::null(),
                            0,
                        ),
                        _RX => libc::recvfrom(
                            inner.fd.as_raw_fd(),
                            ptr::null_mut(),
                            0,
                            libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
                            ptr::null_mut(),
                            ptr::null_mut(),
                        ),
                    }
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
        }
        Ok(())
    }
}