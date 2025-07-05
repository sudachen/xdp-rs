//! # High-Level Packet Sending Logic
//!
//! ## Purpose
//!
//! This file provides the high-level `send` methods for the `TxSocket`. It offers a
//! convenient API for users to send packet data without managing the underlying
//! descriptors and rings directly.
//!
//! ## How it works
//!
//! The `send` method orchestrates the sending process. It first calls `peek` to acquire
//! a writable buffer (a frame in the UMEM) and a corresponding TX descriptor index.
//! It then copies the user's packet data into this buffer. Finally, it calls `commit`
//! to submit the descriptor to the kernel for transmission. It also provides a
//! `send_blocking` variant that waits for the send to complete.
//!
//! ## Main components
//!
//! - `impl Socket<_TX>`: An implementation block for the transmit socket.
//! - `send()`: A non-blocking method to send a slice of data.
//! - `send_blocking()`: A blocking method that sends data and waits for the operation
//!   to be acknowledged by the kernel.

use crate::socket::{RingError,_TX,Socket};

/// An implementation block for the transmit socket (`TxSocket`) that provides
/// high-level sending methods.
impl Socket<_TX> {
    /// Sends a packet in a non-blocking manner.
    ///
    /// This method acquires a buffer from the UMEM, copies the header (if any) and
    /// data into it, and then commits it to the TX ring for the kernel to send.
    /// It will return a `RingError::RingFull` if no space is available.
    ///
    /// # Arguments
    /// * `data` - A byte slice containing the packet payload.
    /// * `header` - An optional byte slice for the packet header. If provided, it is
    ///   prepended to the data.
    ///
    /// # Returns
    /// A `Result` indicating success or a `RingError` on failure.
    pub fn send(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), RingError> {
        let hdr_len = header.map_or(0, |h| h.len());
        let buf_len = data.len() + hdr_len;
        let buf = self.peek(buf_len)?;
        if let Some(bs) = header {
            buf[0..hdr_len].copy_from_slice(bs);
        }
        buf[hdr_len..].copy_from_slice(data);
        self.commit()
    }

    /// Sends a packet and blocks until the operation is complete.
    ///
    /// This method first calls `send` to queue the packet and then calls `poll_wait`
    /// to block until the kernel has processed the send operation.
    ///
    /// # Arguments
    /// * `data` - A byte slice containing the packet payload.
    /// * `header` - An optional byte slice for the packet header.
    ///
    /// # Returns
    /// A `Result` indicating success or a `RingError` on failure.
    pub fn send_blocking(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), RingError> {
        self.send(data, header)?;
        self.poll_wait(None).map_err(RingError::Io)?;
        Ok(())
    }
}
