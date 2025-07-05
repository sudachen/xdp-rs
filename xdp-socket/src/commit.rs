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
    /// Commits a descriptor to the TX ring, making it available for the kernel to send.
    ///
    /// This method should be called after packet data has been written to the UMEM
    /// frame corresponding to the descriptor at `x_head`. It updates the producer
    /// index of the TX ring, signaling to the kernel that a new packet is ready.
    ///
    /// # Arguments
    ///
    /// * `x_head` - The index of the descriptor in the TX ring to commit. This must
    ///   match the current producer head of the ring.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns `RingError::InvalidTxHead` if `x_head` does not match the expected
    /// producer index or if there are no available frames to commit.
    pub fn commit(&mut self, x_head: u32) -> Result<(), RingError> {
        let x_ring = &mut self.x_ring;
        if self.available == 0 || x_head != (self.producer & x_ring.mod_mask) {
            return Err(RingError::InvalidTxHead);
        }
        self.available -= 1;
        self.producer += 1;
        x_ring.update_producer(self.producer);
        Ok(())
    }
}
