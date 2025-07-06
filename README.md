## Motivation

Existing XDP socket **crates** are often too high-level and complex. This **crate** provides a simple, low-level API to control an XDP socket efficiently and without extra overhead.

## API Design

The **crate** provides two main socket types: `TxSocket` for sending (transmitting) data and `RxSocket` for receiving data. A bidirectional socket is handled as a pair of `TxSocket` and `RxSocket`.

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

This project is licensed under MIT license ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
