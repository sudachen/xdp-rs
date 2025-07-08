# xdp-util

[![crates.io](https://img.shields.io/crates/v/xdp-util.svg)](https://crates.io/crates/xdp-util)
[![Documentation](https://docs.rs/xdp-util/badge.svg)](https://docs.rs/xdp-util)

A utility library for XDP (eXpress Data Path) socket operations, networking, and packet
processing in Rust. Provides helpers for netlink communication, packet header construction,
routing, XDP program management, and MAC address lookup.

## Features

- Netlink utilities for querying routes, neighbors, and network interfaces
- Packet header construction (e.g., UDP headers)
- Routing and next-hop resolution
- XDP program management helpers
- MAC address lookup by interface index

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
xdp-util = "<latest-version>"
```

Then import and use the utilities:

```rust
use xdp_util::{get_ipv4_routes, write_udp_header_for, Router, mac_by_ifindex};

let routes = get_ipv4_routes(None)?;
let mac = mac_by_ifindex(2)?;
let mut router = Router::new(2);
router.refresh()?;
```

## Documentation

Full API docs are available at [docs.rs/xdp-util](https://docs.rs/xdp-util).

## Minimum Supported Rust Version

This crate supports Rust 1.64 and above.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Contributions are welcome! Please open issues or pull requests on GitHub.
