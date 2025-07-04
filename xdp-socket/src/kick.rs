use std::{io, ptr};
use std::sync::atomic::Ordering;
use std::os::fd::{AsRawFd as _};
use crate::socket::{_RX, _TX, _Direction, Socket};

impl<const t:_Direction> Socket<t> {
    /// Wakes up the kernel, so it can process packets in the ring.
    ///
    /// The `enforce` parameter is used to force the wake-up call even if the
    /// kernel does not require it. This can be used to ensure the kernel is
    /// aware of packets that were recently added to the ring.
    ///
    /// If the kernel is not aware of the packets, the `XDP_RING_NEED_WAKEUP`
    /// flag will be set in the ring's flags. This method checks this flag and
    /// only performs the wake-up call if it is set or if `enforce` is true.
    ///
    /// If the wake-up call fails, this method will return the error. If the
    /// error is `ENETDOWN`, a warning will be logged, but the method will not
    /// return an error.
    pub fn kick(&self, enforce: bool) -> Result<(), io::Error> {
        if let Some(inner) = &self.inner {
            let need_wakeup = enforce
                || unsafe {
                (*self.x_ring.mmap.flags).load(Ordering::Relaxed) & libc::XDP_RING_NEED_WAKEUP != 0
            };
            if need_wakeup
                && 0 > unsafe {
                match t {
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
                    )
                }
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
        }
        Ok(())
    }

}