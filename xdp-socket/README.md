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
    let mut sok = xdp_socket::create_tx_socket(if_index, 0, None)
        .map_err(|e| io::Error::other(format!("Failed to create XDP socket: {e}")))?;

    let mut bf = sok.seek_and_peek(42 + bytes.len()).map_err(|e|
        io::Error::other(format!("Failed to seek and peek: {e}")))?;

    PacketBuilder::ethernet2(src_mac, next_hop.mac_addr.unwrap())
        .ipv4(src.ip().octets(), dst.ip().octets(), 64) // 64 is a common TTL
        .udp(src.port(), dst.port())
        .write(&mut bf, bytes)
        .map_err(|e| io::Error::other(format!("Error writing packet header: {e}")))?;

    sok.commit().map_err(|e| io::Error::other( format!("Failed to commit buffer in RX ring: {e}")))?;
    sok.kick()?;
```

## Safety

This crate is inherently `unsafe` because creating and managing AF_XDP sockets requires direct interaction with the Linux kernel through low-level APIs (`libc`). The caller is responsible for ensuring:

1.  The application has the necessary capabilities (e.g., `CAP_NET_ADMIN`, `CAP_NET_RAW`, `CAP_BPF`) to create AF_XDP sockets.
2.  The provided network interface index and queue ID are valid.
3.  Memory is handled correctly, although the library provides safe abstractions where possible.

## License

This project is licensed under either of

- MIT license ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))

at your option.
