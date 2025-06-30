#![cfg(test)]

use std::io::Write;
use std::net::Ipv4Addr;
use etherparse::{SlicedPacket, TransportSlice};
use crate::write_udp_header_for;

#[test]
fn test_write_udp_header() {
    let src_addr = Ipv4Addr::new(192, 168, 1, 1);
    let dst_addr = Ipv4Addr::new(192, 168, 1, 2);
    let src_mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
    let dst_mac = [0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb];
    let src_port = 12345;
    let dst_port = 54321;
    let data = b"Hello, XDP!";
    let hdr = write_udp_header_for(data, src_addr, src_mac, src_port, dst_addr, dst_mac, dst_port).unwrap();
    assert_eq!(hdr.len(), 42);
    let mut buf = [0u8; 42+11];
    buf[..42].clone_from_slice(&hdr);
    buf[42..].copy_from_slice(data);
    match SlicedPacket::from_ethernet(&buf) {
        Ok(packet) => {
            match packet.transport {
                Some(TransportSlice::Udp(_udp)) => {},
                _ => panic!("Not udp packet"),
            }
        },
        Err(e) => panic!("Failed to parse packet: {}", e),
    };
}

#[test]
fn test_hdrwrite() {
    let mut hdr = [0u8; 42];
    let data = b"Test data";
    let written = {
        let mut writer = crate::packet::HdrWrite(&mut hdr, 0);
        writer.write(data).unwrap()
    };
    assert_eq!(written, data.len());
    assert_eq!(&hdr[..written], data);
    let data = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaTest data";
    let written = {
        let mut writer = crate::packet::HdrWrite(&mut hdr, 0);
        writer.write(data).unwrap()
    };
    assert_eq!(written, data.len());
    assert_eq!(&hdr[..], &data[..42]);
}