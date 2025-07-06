# `xdp-socket`: A Low-Level Rust Library for AF_XDP Sockets

[![Crates.io](https://img.shields.io/crates/v/xdp-socket.svg)](https://crates.io/crates/xdp-socket)
[![Docs.rs](https://docs.rs/xdp-socket/badge.svg)](https://docs.rs/xdp-socket)

This crate provides a simple, low-level, and efficient Rust API for working with AF_XDP sockets on Linux. It is designed for high-performance networking applications that require direct control and minimal overhead.

## Motivation

Many existing libraries for AF_XDP sockets are high-level and opinionated, which can introduce unnecessary complexity and overhead. `xdp-socket` takes a different approach by offering a thin, unopinionated wrapper around the raw AF_XDP interface, giving developers the control they need for demanding, low-latency applications.

## Features

- **Low-Level Control**: Directly manage AF_XDP sockets, rings, and UMEM regions.
- **High Performance**: Designed for minimal overhead, enabling applications to achieve line-rate packet processing on isolated CPU cores.
- **Directional Sockets**: Clear and type-safe distinction between transmit-only (`TxSocket`) and receive-only (`RxSocket`) sockets.
- **Zero-Copy Forwarding**: Full support for zero-copy data transfers if the network interface driver has support.
- **Configurable**: Options to manage huge pages, kernel wakeup notifications, and zero-copy behavior.

## API Design

The library provides two main socket types:
- `TxSocket`: For sending (transmitting) packets.
- `RxSocket`: For receiving packets.

A bidirectional socket can be created as a pair of `(TxSocket, RxSocket)` that share the same underlying UMEM.

### Sending Packets (`TxSocket`)

The `TxSocket` provides a straightforward `send` method. Internally, it manages a `seek`/`peek`/`commit` workflow to find an available frame in the UMEM, allow you to write data to it, and submit it to the kernel for transmission.

### Receiving Packets (`RxSocket`)

The `RxSocket` is designed to efficiently receive packets from the network. The API allows you to poll for received packets and process them directly from the UMEM.

## Usage

First, add `xdp-socket` to your `Cargo.toml` dependencies:

```toml
[dependencies]
xdp-socket = "0.1.0" # Replace with the latest version
```

Here is a basic example of how to create a `TxSocket` and send a UDP packet:

```rust
use std::io;
use xdp_socket::{create_tx_socket, Direction, XdpConfig};

fn main() -> io::Result<()> {
    // Note: This example requires running with root privileges to create AF_XDP sockets
    // and assumes a network interface named `eth0` exists.

    let if_index = match nix::net::if_::if_nametoindex("eth0") {
        Ok(index) => index,
        Err(_) => {
            eprintln!("Network interface 'eth0' not found.");
            return Ok(());
        }
    };
    let if_queue = 0;

    // Create a transmit-only socket
    let config = XdpConfig {
        zero_copy: Some(true),
        need_wakeup: Some(true),
        ..Default::default()
    };

    let (mut tx_socket, _) = match create_tx_socket(if_index, if_queue, Some(config)) {
        Ok(socket) => socket,
        Err(e) => {
            eprintln!("Failed to create TX socket: {}", e);
            return Err(e);
        }
    };

    // The packet data to be sent
    let packet_data = b"Hello, XDP!";

    println!("Sending a packet...");

    // Send the packet
    if let Err(e) = tx_socket.send(packet_data, None) {
        eprintln!("Failed to send packet: {:?}", e);
    }

    println!("Packet sent successfully!");

    Ok(())
}
```

## Advanced Usage: Manual Frame Management

For more fine-grained control, you can use the `peek` and `commit` methods to manage frames manually. This is useful when you need to prepare data in the UMEM buffer before deciding to send it.

```rust
use std::io;
use xdp_socket::{create_tx_socket, Direction};

fn main() -> io::Result<()> {
    // Note: This example requires running with root privileges.
    let if_index = xdp_socket::util::name_to_index("eth0").expect("interface not found");

    // Create a transmit-only socket with default configuration by passing `None`
    let (mut tx_socket, _) = create_tx_socket(if_index, 0, None)
        .expect("failed to create socket");

    let packet_data = b"Hello again, XDP!";
    let hdr = xdp_socket::util::write_udp_header_for(
        packet_data,
        src_addr,
        src_mac,
        src_port,
        dst_addr,
        next_hop.mac_addr.unwrap(),
        dst_port,
    )?;
    
    println!("Sending a packet using seek/peek/commit API...");

    // 1. Peek for an available frame and get a writable buffer.
    //    `seek_and_peek` finds an available frame and returns a mutable slice to its
    //    buffer and the pointer to the len field in the Tx descriptor.
    match tx_socket.seek_and_peek() {
        Ok((buf, buf_len)) => {
            *buf_len = PacketBuilder::ethernet2(src_mac, dst_mac)
                .ipv4(src_addr.octets(), dst_addr.octets(), 64) // 64 is a common TTL
                .udp(src_port, dst_port)
                .payload(packet_data)
                .write_to(&mut *buf);

            // 3. Commit the frame for transmission
            if let Err(e) = tx_socket.commit(tx_head) {
                eprintln!("Failed to commit packet: {:?}", e);
                return Ok(());
            }

            // 4. Kick the kernel to ensure it processes the packet immediately.
            //    The `enforce` flag is set to `true` to force a syscall.
            if let Err(e) = tx_socket.kick(true) {
                eprintln!("Failed to kick kernel: {:?}", e);
            } else {
                println!("Packet sent successfully using low-level API!");
            }
        }
        Err(e) => {
            eprintln!("Failed to peek for a frame: {:?}", e);
        }
    }
    Ok(())
}
```

## Safety

This crate is inherently `unsafe` because creating and managing AF_XDP sockets requires direct interaction with the Linux kernel through low-level APIs (`libc`). The caller is responsible for ensuring:

1.  The application has the necessary capabilities (e.g., `CAP_NET_ADMIN`) to create AF_XDP sockets.
2.  The provided network interface index and queue ID are valid.
3.  Memory is handled correctly, although the library provides safe abstractions where possible.

## License

This project is licensed under either of

- MIT license ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))

at your option.
