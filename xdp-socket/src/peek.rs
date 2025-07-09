//! # Peeking at Descriptors in XDP Rings
//!
//! ## Purpose
//!
//! This file provides the logic for "peeking" at descriptors in an XDP ring.
//! Peeking allows the application to get a reference to the data buffer (UMEM
//! frame) associated with a descriptor without advancing the ring's consumer or
//! producer indices. This is useful for inspecting or modifying data before
//! committing to sending it (for TX) or after receiving it but before releasing
//! the descriptor (for RX).
//!
//! ## How it works
//!
//! It implements methods on `Socket<_TX>` and `Socket<_RX>` that allow access
//! to the underlying UMEM data buffers.
//!
//! For `_TX`, the methods return a mutable slice (`&mut [u8]`) to a UMEM frame,
//! allowing the application to write packet data into the buffer before it is
//! sent. The `len` of the data to be sent must be specified.
//!
//! For `_RX`, the methods return a slice (`&[u8]`) to a UMEM frame containing
//! a received packet, allowing the application to read the data.
//!
//! ## Main components
//!
//! - `impl Socket<_TX>`: Provides `peek`, `peek_at`, and `seek_and_peek` for
//!   transmit sockets.
//! - `impl Socket<_RX>`: Provides `peek`, `peek_at`, and `seek_and_peek` for
//!   receive sockets.

#![allow(private_interfaces)]
#![allow(private_bounds)]

use crate::socket::{_RX, _TX, RingError, Seek_, Socket};

impl Socket<_TX>
where
    Socket<_TX>: Seek_<_TX>,
{
    /// Peeks at the `index`-th available chunk in the ring without advancing the head.
    ///
    /// This function finds the `index`-th descriptor in the range of AVAILABLE descriptors
    /// and returns a mutable slice into the UMEM for writing (TX).
    ///
    /// # Arguments
    ///
    /// * `index` - The index in the range of available descriptors.
    /// * `len` - The desired length of the chunk.
    ///
    /// # Returns
    ///
    /// A `Result` containing a mutable byte slice and its corresponding descriptor index.
    fn peek_(&mut self, index: usize, len: usize) -> Result<&mut [u8], RingError> {
        #[cfg(not(feature = "no_safety_checks"))]
        if index >= self.available as usize {
            return Err(RingError::InvalidIndex);
        }
        #[cfg(not(feature = "no_safety_checks"))]
        if len > self.x_ring.frame_size() as usize {
            return Err(RingError::InvalidLength);
        }
        let x_head = self.producer.wrapping_add(index as u32) & self.x_ring.mod_mask;
        self.x_ring.mut_desc_at(x_head).len = len as u32;
        Ok(self.x_ring.mut_bytes_at(self.frames, x_head, len))
    }

    /// Peeks at the next available chunk in the ring without advancing the head.
    ///
    /// This function finds the next available descriptor using `seek` and returns a
    /// mutable slice into the UMEM for writing (TX) or reading (RX).
    ///
    /// # Arguments
    ///
    /// * `len` - The desired length of the chunk.
    ///
    /// # Returns
    ///
    /// A `Result` containing a mutable byte slice and its corresponding descriptor index.
    #[inline]
    pub fn peek(&mut self, len: usize) -> Result<&mut [u8], RingError> {
        self.peek_(0, len)
    }

    /// Peeks at the `index`-th available chunk in the ring without advancing the head.
    ///
    /// This function finds the `index`-th descriptor in the range of available descriptors
    /// and returns a mutable slice into the UMEM for writing (TX).
    ///
    /// # Arguments
    ///
    /// * `index` - The index in the range of available descriptors.
    /// * `len` - The desired length of the chunk.
    ///
    /// # Returns
    ///
    /// A `Result` containing a mutable byte slice and its corresponding descriptor index.
    #[inline]
    pub fn peek_at(&mut self, index: usize, len: usize) -> Result<&mut [u8], RingError> {
        self.peek_(index, len)
    }

    /// Seeks to the next available descriptor in the ring and peeks at the descriptor
    /// without advancing the head.
    ///
    /// This function calls `seek_` with a count of 1, and then calls `peek_` with a length
    /// of `len`. It returns a mutable slice into the UMEM for writing (TX).
    ///
    /// # Arguments
    ///
    /// * `len` - The desired length of the chunk.
    ///
    /// # Returns
    ///
    /// A `Result` containing a mutable byte slice and its corresponding descriptor index.
    pub fn seek_and_peek(&mut self, len: usize) -> Result<&mut [u8], RingError> {
        self.seek_(1)?;
        self.peek_(0, len)
    }
}

impl Socket<_RX>
where
    Socket<_RX>: Seek_<_RX>,
{
    /// Peeks at the `index`-th available chunk in the ring without advancing the head.
    ///
    /// This function finds the `index`-th descriptor in the range of available descriptors
    /// and returns a byte slice into the UMEM for reading (RX).
    ///
    /// # Arguments
    ///
    /// * `index` - The index in the range of available descriptors.
    ///
    /// # Returns
    ///
    /// A `Result` containing a byte slice and its corresponding descriptor index.
    fn peek_(&mut self, index: usize) -> Result<&[u8], RingError> {
        #[cfg(not(feature = "no_safety_checks"))]
        if index >= self.available as usize {
            return Err(RingError::InvalidIndex);
        }
        let x_head = self.consumer.wrapping_add(index as u32) & self.x_ring.mod_mask;
        let len = self.x_ring.desc_at(x_head).len as usize;
        Ok(self.x_ring.mut_bytes_at(self.frames, x_head, len))
    }
    /// Peeks at the first available chunk in the ring without advancing the head.
    ///
    /// This function finds the first descriptor in the range of available descriptors
    /// and returns a byte slice into the UMEM for reading (RX).
    ///
    /// # Returns
    ///
    /// A `Result` containing a byte slice and its corresponding descriptor index.
    #[inline]
    pub fn peek(&mut self) -> Result<&[u8], RingError> {
        self.peek_(0)
    }

    /// Peeks at the `index`-th available chunk in the ring without advancing the head.
    ///
    /// This function finds the `index`-th descriptor in the range of available descriptors
    /// and returns a byte slice into the UMEM for reading (RX).
    ///
    /// # Arguments
    ///
    /// * `index` - The index in the range of available descriptors.
    ///
    /// # Returns
    ///
    /// A `Result` containing a byte slice and its corresponding descriptor index.
    #[inline]
    pub fn peek_at(&mut self, index: usize) -> Result<&[u8], RingError> {
        self.peek_(index)
    }

    /// Seeks to the next available descriptor in the ring and peeks at the descriptor
    /// without advancing the head.
    ///
    /// This function calls `seek_` with a count of 1, and then calls `peek_` with a length
    /// of the descriptor length. It returns a byte slice into the UMEM for reading (RX).
    ///
    /// # Returns
    ///
    /// A `Result` containing a byte slice and its corresponding descriptor index.
    pub fn seek_and_peek(&mut self) -> Result<&[u8], RingError> {
        self.seek_(1)?;
        self.peek_(0)
    }
}
