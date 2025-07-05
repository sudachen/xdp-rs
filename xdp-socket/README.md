# xdp-socket

`xdp-socket` is a Rust crate that provides a high-level interface for `AF_XDP` sockets on Linux, enabling high-performance networking through zero-copy data transfers. It is built on top of `libbpf-sys` and offers a safe, idiomatic Rust API for working with XDP.

This crate is part of the `xdp-rs` project.

## Features

- **High-Performance:** Zero-copy packet processing for both sending (TX) and receiving (RX).
- **Safe API:** Provides a safe Rust interface over the underlying `libbpf` and kernel APIs.
- **Flexible Configuration:** Supports flexible socket configuration, including UMEM, ring buffer sizes, and more.
- **Bidirectional Sockets:** Create sockets for TX, RX, or both.

## Usage

Add `xdp-socket` to your `Cargo.toml` dependencies:

```toml
[dependencies]
xdp-socket = "0.1.0"
```

For more detailed examples, please refer to the tests in the `tests` directory.

## Running Tests

To run the tests for this crate, you will need to have the `libbpf` development headers installed on your system. You can then run the tests using Cargo:

```sh
cargo test --workspace
```

Some tests may require root privileges to run.

## License

This project is licensed under the MIT License. See the [LICENSE.md](../LICENSE.md) file for details.

## Contribution

Contributions are welcome! Please feel free to open an issue or submit a pull request.