
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
