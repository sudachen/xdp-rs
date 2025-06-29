use crate::socket::{AfXdpSocket, Direction};
use std::os::fd::AsRawFd as _;
use std::sync::atomic::Ordering;
use std::{io, ptr};

pub struct Transmitter<'a>(&'a AfXdpSocket);

impl AfXdpSocket {
    pub fn tx(&self) -> Result<Transmitter<'_>, io::Error> {
        if self.direction == Direction::Rx {
            return Err(io::Error::other("Cannot send on a receive-only socket"));
        }
        Ok(Transmitter(self))
    }
}

impl Transmitter<'_> {
    pub fn send(&self, _data: &[u8], _header: Option<&[u8]>) -> Result<(), io::Error> {
        self.tx_wakeup()?;

        Ok(())
    }
    pub fn tx_wakeup(&self) -> Result<(), io::Error> {
        let need_wakeup = unsafe {
            (*self.0.tx_ring.mmap.flags).load(Ordering::Relaxed) & libc::XDP_RING_NEED_WAKEUP != 0
        };
        if need_wakeup
            && 0 > unsafe {
                libc::sendto(
                    self.0.fd.as_raw_fd(),
                    ptr::null(),
                    0,
                    libc::MSG_DONTWAIT,
                    ptr::null(),
                    0,
                )
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
        Ok(())
    }
}
