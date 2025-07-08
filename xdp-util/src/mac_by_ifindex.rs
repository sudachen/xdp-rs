
use std::io;

/// Returns the MAC address of the network interface with the given index, or
/// an error if the operation fails.
///
/// This function uses the `SIOCGIFNAME` and `SIOCGIFHWADDR` ioctls to retrieve
/// the MAC address. It is not available on all platforms, and may fail for
/// various reasons, such as the interface not existing or not supporting the
/// operation.
///
/// # Safety
///
/// This function is unsafe because it calls the `ioctl` syscall, which is a
/// low-level, platform-specific operation. It is also possible that the
/// interface may be modified or removed while the function is running,
/// causing the function to fail.
pub fn mac_by_ifindex(if_index: u32) -> Result<[u8;6], io::Error> {
    unsafe {
        let socket_fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if socket_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut if_req: libc::ifreq = std::mem::zeroed();
        if_req.ifr_ifru.ifru_ifindex = if_index as libc::c_int;
        if libc::ioctl(socket_fd, libc::SIOCGIFNAME, &mut if_req) < 0 {
            libc::close(socket_fd);
            return Err(io::Error::last_os_error());
        }
        if_req.ifr_ifru.ifru_ifindex = 0;
        if libc::ioctl(socket_fd, libc::SIOCGIFHWADDR, &mut if_req) < 0 {
            libc::close(socket_fd);
            return Err(io::Error::last_os_error());
        }
        libc::close(socket_fd);
        let mut result = [0u8;6];
        for (i, v) in if_req.ifr_ifru.ifru_hwaddr.sa_data[..6].iter().enumerate() {
            result[i] = *v as u8;
        }
        Ok(result)
    }
}
