//! # UDP Packet Header Construction
//!
//! ## Purpose
//!
//! This module provides a utility to construct the headers for a UDP/IPv4/Ethernet
//! packet. This is essential for applications using AF_XDP that need to manually
//! build entire packets before sending them.
//!
//! ## How it works
//!
//! It uses the `etherparse` crate's `PacketBuilder` to efficiently layer the Ethernet II,
//! IPv4, and UDP headers. The headers are written directly into a fixed-size `[u8; 42]`
//! array to avoid heap allocations. A custom `HdrWrite` struct, which implements
//! `std::io::Write`, acts as a temporary writer to capture the builder's output.
//!
//! ## Main components
//!
//! - `write_udp_header_for()`: The primary function that takes source/destination
//!   addresses and ports and returns a 42-byte array containing the packet header.
//! - `HdrWrite`: A helper struct implementing `io::Write` to enable writing into a
//!   fixed-size buffer without extra allocations.

use etherparse::PacketBuilder;
use std::io;
use std::net::Ipv4Addr;

/// Constructs Ethernet, IPv4, and UDP headers for a packet.
///
/// This function uses `etherparse::PacketBuilder` to create the necessary headers
/// and writes them into a 42-byte array.
///
/// # Arguments
/// * `data` - The payload data, used to calculate the UDP payload length.
/// * `src_addr` - Source IPv4 address.
/// * `src_mac` - Source MAC address.
/// * `src_port` - Source UDP port.
/// * `dst_addr` - Destination IPv4 address.
/// * `dst_mac` - Destination MAC address.
/// * `dst_port` - Destination UDP port.
///
/// # Returns
/// A `Result` containing a 42-byte array with the complete L2/L3/L4 headers,
/// or an `io::Error` on failure.
pub fn write_udp_header_for(
    data: &[u8],
    src_addr: Ipv4Addr,
    src_mac: [u8; 6],
    src_port: u16,
    dst_addr: Ipv4Addr,
    dst_mac: [u8; 6],
    dst_port: u16,
) -> io::Result<[u8; 42]> {
    let mut hdr = [0u8; 42]; // 14 bytes for Ethernet header + 20 bytes for IPv4 header + 8 bytes for UDP header

    let builder = PacketBuilder::
    // Layer 2: Ethernet II header
    ethernet2(src_mac, dst_mac)
    // Layer 3: IPv4 header
    .ipv4(src_addr.octets(), dst_addr.octets(), 64) // 64 is a common TTL
    // Layer 4: UDP header
    .udp(src_port, dst_port);

    match builder.write(&mut HdrWrite(&mut hdr, 0), data) {
        Ok(_) => Ok(hdr),
        Err(e) => Err(io::Error::other(format!(
            "Error writing packet header: {e}",
        ))),
    }
}

/// A helper struct that implements `std::io::Write` for a fixed-size byte array.
///
/// This is used to capture the output of `etherparse::PacketBuilder` without
/// requiring heap allocations.
pub struct HdrWrite<'a>(
    /// The buffer to write into.
    pub &'a mut [u8; 42],
    /// The current write position within the buffer.
    pub usize,
);
impl io::Write for HdrWrite<'_> {
    /// Writes a buffer into the inner array, respecting the fixed size.
    ///
    /// It copies bytes from `buf` into the internal array. If `buf` is larger
    /// than the remaining space, the data is truncated.
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        if self.1 < 42 {
            let len = buf.len().min(self.0.len() - self.1);
            self.0[self.1..self.1 + len].copy_from_slice(&buf[..len]);
        }
        self.1 += buf.len();
        Ok(buf.len())
    }

    /// A no-op flush implementation, as the write is immediate.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

//
// ================================================================================================
//   UNITTESTS
// ================================================================================================
//
#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use super::HdrWrite;
    use std::io::Write;

    #[test]
    fn test_write_udp_header() {
        let src_addr = Ipv4Addr::new(192, 168, 1, 1);
        let dst_addr = Ipv4Addr::new(192, 168, 1, 2);
        let src_mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let dst_mac = [0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb];
        let src_port = 12345;
        let dst_port = 54321;
        let data = b"Hello, XDP!";
        let hdr = super::write_udp_header_for(
            data, src_addr, src_mac, src_port, dst_addr, dst_mac, dst_port,
        )
            .unwrap();
        assert_eq!(hdr.len(), 42);
        let mut buf = [0u8; 42 + 11];
        buf[..42].clone_from_slice(&hdr);
        buf[42..].copy_from_slice(data);
        match etherparse::SlicedPacket::from_ethernet(&buf) {
            Ok(packet) => match packet.transport {
                Some(etherparse::TransportSlice::Udp(_udp)) => {}
                _ => panic!("Not udp packet"),
            },
            Err(e) => panic!("Failed to parse packet: {}", e),
        };
    }

    #[test]
    fn test_hdrwrite() {
        let mut hdr = [0u8; 42];
        let data = b"Test data";
        let written = {
            let mut writer = HdrWrite(&mut hdr, 0);
            writer.write(data).unwrap()
        };
        assert_eq!(written, data.len());
        assert_eq!(&hdr[..written], data);
        let data = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaTest data";
        let written = {
            let mut writer = HdrWrite(&mut hdr, 0);
            writer.write(data).unwrap()
        };
        assert_eq!(written, data.len());
        assert_eq!(&hdr[..], &data[..42]);
    }
}
