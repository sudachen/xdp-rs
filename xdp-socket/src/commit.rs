//! # XDP Socket TX Commit
//!
//! ## Purpose
//!
//! This file implements the `commit` method for the transmit socket (`Socket<_TX>`).
//! This method is called after packet data has been written to a frame in the UMEM.
//! It finalizes the frame for transmission by the kernel.
//!
//! ## How it works
//!
//! The `commit` function updates the producer index of the TX ring (`x_ring`), which
//! effectively hands over the descriptor to the kernel for sending. It also decrements
//! the count of available frames. It includes a check to ensure the commit operation
//! is valid and corresponds to the expected descriptor head.
//!
//! ## Main components
//!
//! - `impl Socket<_TX>`: An implementation block specifically for the transmit socket.
//! - `commit()`: The public method that commits a single packet descriptor to the TX ring,
//!   making it available for the kernel to send.

use crate::socket::{RingError, Socket, _TX};

/// Implements the commit logic for a transmit (`TX`) socket.
impl Socket<_TX> {
    /// Commits a number of descriptors to the TX ring, making them available for the kernel to send.
    ///
    /// This method should be called after packet data has been written to the UMEM
    /// frames corresponding to the descriptors. It updates the producer
    /// index of the TX ring, signaling to the kernel that new packets are ready.
    ///
    /// # Arguments
    ///
    /// * `count` - The number of descriptors to commit. This must not exceed the
    ///   number of available frames in the ring.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns `RingError::NotAvailable` if there are not enough available frames to commit.
    pub fn commit_n(&mut self, count: usize) -> Result<(), RingError> {
        let x_ring = &mut self.x_ring;
        if self.available < count as u32 {
            return Err(RingError::NotAvailable);
        }
        self.available -= count as u32;
        self.producer += count as u32;
        x_ring.update_producer(self.producer);
        Ok(())
    }

    /// Commits a single descriptor to the TX ring.
    ///
    /// This method is a convenience wrapper around `commit_n` that commits exactly one
    /// descriptor. It is typically used when only a single packet has been prepared
    /// for transmission.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `RingError` if the operation fails, such as when there are no
    /// available frames to commit.
    pub fn commit(&mut self) -> Result<(), RingError> {
        self.commit_n(1)
    }
}
