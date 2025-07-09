//! # High-Level Packet Sending Logic
//!
//! ## Purpose
//!
//! This file provides the high-level `send` methods for the `TxSocket`. It offers a
//! convenient API for users to send packet data without managing the underlying
//! descriptors and rings directly.
//!
//! Since send and send_blocking is not a part of core API,
//!   you have to import Send trait to enable it.
//!
//! ## How it works
//!
//! The user must first call `seek` or `seek_n` to ensure one or more UMEM frames
//! are available for writing. The `send` method then takes the user's packet data,
//! copies it into the next available frame, and calls `commit` to submit the
//! descriptor to the kernel for transmission. It also provides a `send_blocking`
//! variant that waits for the send to complete.
//!
//! ## Main components
//!
//! - `impl Socket<_TX>`: An implementation block for the transmit socket.
//! - `send()`: A non-blocking method to send a slice of data.
//! - `send_blocking()`: A blocking method that sends data and waits for the operation
//!   to be acknowledged by the kernel.

use crate::socket::{RingError,_TX,Socket, Seek_, Commit_};
use crate::poll::PollWaitExt;

/// A trait for high-level packet sending operations on XDP transmit sockets.
///
/// This trait provides methods to send packet data using an AF_XDP socket. It abstracts
/// away the details of descriptor management and UMEM frame handling, offering a simple
/// interface for non-blocking and blocking sends.
///
/// - `send`: Sends a packet in a non-blocking manner. You must ensure a frame is available
///   by calling `seek` or `seek_n` before use.
/// - `send_blocking`: Sends a packet and blocks until the kernel has processed the send.
///
/// # Arguments
///
/// * `data` - A byte slice containing the packet payload.
/// * `header` - An optional byte slice for the packet header.
///
/// # Errors
///
/// Returns a `RingError` if the send fails or if no frame is available.
///
/// # Example
///
/// ```rust
/// use xdp_socket::{create_tx_socket, SendExt};
/// let mut tx = create_tx_socket(...)?;
/// tx.send(b"hello", None)?;
/// ```
pub trait SendExt {
    fn send(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), RingError>;
    fn send_blocking(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), RingError>;
}

/// An implementation block for the transmit socket (`TxSocket`) that provides
/// high-level sending methods.
impl SendExt for Socket<_TX> where Socket<_TX>: Seek_<_TX> + Commit_<_TX> + PollWaitExt<_TX> {

    /// Sends a packet in a non-blocking manner.
    ///
    /// This method copies the provided data into a UMEM frame that has been
    /// previously acquired via a call to `seek` or `seek_n`, and then submits it
    /// to the kernel for transmission.
    ///
    /// Before calling this function, you must ensure that a frame is available by
    /// calling `seek` or `seek_n`.
    ///
    /// # Arguments
    /// * `data` - A byte slice containing the packet payload.
    /// * `header` - An optional byte slice for the packet header. If provided, it is
    ///   prepended to the data.
    ///
    /// # Returns
    /// A `Result` indicating success or a `RingError` on failure.
    ///
    /// # Errors
    ///
    /// Returns `RingError::InvalidLength` if `data.len() + header.len()` exceeds
    /// the UMEM frame size. Returns `RingError::InvalidIndex` if `seek` has not
    /// been called to make a frame available.
    fn send(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), RingError> {
        let hdr_len = header.map_or(0, |h| h.len());
        let buf_len = data.len() + hdr_len;
        let buf = self.peek(buf_len)?;
        if let Some(bs) = header {
            buf[0..hdr_len].copy_from_slice(bs);
        }
        buf[hdr_len..].copy_from_slice(data);
        self.commit()
    }

    /// Sends a packet and blocks until the kernel has processed the send.
    ///
    /// This method first calls `send` to queue the packet and then blocks, waiting
    /// for a kernel notification that the send is complete.
    ///
    /// Before calling this function, you must ensure that a frame is available by
    /// calling `seek` or `seek_n`.
    ///
    /// # Arguments
    /// * `data` - A byte slice containing the packet payload.
    /// * `header` - An optional byte slice for the packet header.
    ///
    /// # Returns
    /// A `Result` indicating success or a `RingError` on failure.
    ///
    /// # Errors
    ///
    /// In addition to the errors from `send`, this function can return
    /// `RingError::Io` if the underlying `poll_wait` fails.
    fn send_blocking(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), RingError> {
        self.send(data, header)?;
        self.poll_wait(None).map_err(RingError::Io)?;
        Ok(())
    }
}
