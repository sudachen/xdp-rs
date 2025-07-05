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
//! The `commit_` function updates the producer index of the TX ring (`x_ring`), which
//! effectively hands over the descriptor to the kernel for sending. It also decrements
//! the count of available frames. It includes a check to ensure the commit operation
//! is valid and corresponds to the expected descriptor head.
//!
//! ## Main components
//!
//! - `impl Socket<_TX>`: An implementation block specifically for the transmit socket.
//! - `commit()`: The public method that commits a single packet descriptor to the TX ring,
//!   making it available for the kernel to send.

use crate::socket::{RingError, Socket, Commit_, _TX, _RX};

/// Implements the commit logic for a transmit (`TX`) socket.
impl Commit_<_TX> for Socket<_TX> {
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
    fn commit_(&mut self, count: usize) -> Result<(), RingError> {
        #[cfg(not(feature="no_safety_checks"))]
        if self.available < count as u32 {
            return Err(RingError::NotAvailable);
        }
        self.available -= count as u32;
        self.producer = self.producer.wrapping_add(count as u32);
        self.x_ring.update_producer(self.producer);
        Ok(())
    }
}

impl Commit_<_RX> for Socket<_RX> {
    fn commit_(&mut self, count: usize) -> Result<(), RingError> {
        #[cfg(not(feature="no_safety_checks"))]
        if self.available < count as u32 {
            return Err(RingError::NotAvailable);
        }
        let f_ring = &mut self.u_ring;
        let x_ring = &mut self.x_ring;
        for _ in 0..(count as u32) {
            let addr = x_ring.desc_at(self.consumer & x_ring.mod_mask).addr;
            *f_ring.mut_desc_at(self.producer & x_ring.mod_mask) = addr;
            self.consumer = self.consumer.wrapping_add(1);
            self.producer = self.producer.wrapping_add(1);
        }
        self.available -= count as u32;
        x_ring.update_consumer(self.consumer);
        f_ring.update_producer(self.producer);
        Ok(())
    }
}
