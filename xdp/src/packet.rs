use std::net::Ipv4Addr;
use std::io;
use etherparse::{PacketBuilder};

pub fn write_udp_header_for(data: &[u8], src_addr: Ipv4Addr, src_mac: [u8;6], src_port: u16, dst_addr: Ipv4Addr, dst_mac: [u8;6], dst_port: u16) -> io::Result<[u8;42]> {
    let mut hdr = [0u8; 42]; // 14 bytes for Ethernet header + 20 bytes for IPv4 header + 8 bytes for UDP header

    let builder = PacketBuilder::
    // Layer 2: Ethernet II header
    ethernet2(src_mac, dst_mac)
        // Layer 3: IPv4 header
        .ipv4(src_addr.octets(), dst_addr.octets(), 64) // 64 is a common TTL
        // Layer 4: UDP header
        .udp(src_port, dst_port);

    match builder.write(&mut HdrWrite(&mut hdr,0), data) {
        Ok(_) => Ok(hdr),
        Err(e) => {
            Err(io::Error::other(format!("Error writing packet header: {}", e)))
        }
    }
}

pub struct HdrWrite<'a>(pub &'a mut [u8;42], pub usize);
impl io::Write for HdrWrite<'_> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        if self.1 < 42 {
            let len = buf.len().min(self.0.len() - self.1);
            self.0[self.1..self.1+len].copy_from_slice(&buf[..len]);
        }
        self.1 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> { Ok(())}
}
