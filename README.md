## XDP-Socket

This crate provides a simple and transparent Rust implementation of AF_XDP sockets. It is designed for applications that require direct, high-performance access to network interfaces, bypassing the kernel's networking stack to minimize syscalls and scheduler overhead.

The core design philosophy is a minimalistic API, making it a flexible building block for integration with modern asynchronous ecosystems like tokio, mio, and quinn.

The primary motivation for xdp-socket is to provide a networking foundation for building low-latency and high-throughput applications, with a particular focus on real-time Web3 infrastructure, such as:

 - Peer-to-peer (P2P) data propagation layers

 - High-performance RPC gateways

 - Real-time indexing services

## API Design

There are two main socket types: `TxSocket` for sending (transmitting) data and `RxSocket` for receiving data. A bidirectional socket is handled as a pair of `TxSocket` and `RxSocket`.

Instead of a basic `send`/`recv` model, the main API uses a `seek`/`peek`/`commit` workflow. This gives you direct control over memory and how packets are handled. The behavior of these functions changes depending on whether you are sending or receiving.

#### Sending with `TxSocket` ➡️
1.  **`seek`**: Finds an empty memory frame available for you to write a packet into.
2.  **`peek`**: Gets a writable buffer for that frame.
3.  **`commit`**: Submits the written buffer to the network driver to be sent.

#### Receiving with `RxSocket` ⬅️
1.  **`seek`**: Finds a frame that has already received a packet from the network.
2.  **`peek`**: Gets a readable buffer so you can process the packet's data.
3.  **`commit`**: Releases the frame, allowing it to be reused for receiving new packets.

A batching API (`seek_n`, `peek_at`, `commit_n`) is also available for both sending and receiving, which allows you to process multiple frames at once for better efficiency.

## Performance

This API allows an application to run on an isolated CPU core without yielding to the scheduler. By avoiding these context switches, it achieves the high performance and low latency needed for heavy-load applications.

# Usage

First, add `xdp-socket` to your `Cargo.toml` dependencies:

```toml
[dependencies]
xdp-socket = "0.1" # Replace with the latest version
```

Here is a basic example of how to create a `TxSocket` and send a UDP packet:

```rust
    let mut sok = xdp_socket::create_tx_socket(if_index, 0, None)
        .map_err(|e| io::Error::other(format!("Failed to create XDP socket: {e}")))?;

    let mut buf = sok.seek_and_peek(raw_packet_bytes_len).map_err(|e|
        io::Error::other(format!("Failed to seek and peek: {e}")))?;

    // write packet data into the buffer

    sok.commit().map_err(|e| io::Error::other( format!("Failed to commit buffer in RX ring: {e}")))?;
    sok.kick()?;
```

## Safety

This crate is inherently `unsafe` because creating and managing AF_XDP sockets requires direct interaction with the Linux kernel through low-level APIs (`libc`). The caller is responsible for ensuring:

1.  The application has the necessary capabilities (e.g., `CAP_NET_ADMIN`, `CAP_NET_RAW`, `CAP_BPF`) to create AF_XDP sockets.
2.  The provided network interface index and queue ID are valid.
3.  Memory is handled correctly, although the library provides safe abstractions where possible.

## License

Licensed under either of the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE) at your discretion. This project is dual-licensed to be compatible with the Rust project's licensing scheme and to give users maximum flexibility.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual-licensed as above, without any additional terms or conditions.