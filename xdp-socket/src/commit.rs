//! # Descriptor Committing in XDP Rings
//!
//! ## Purpose
//!
//! This file implements the logic for "committing" descriptors in an XDP ring.
//! Committing finalizes an operation and makes the descriptor available to the
//! kernel.
//!
//! ## How it works
//!
//! It implements the `Commit_` trait for both `Socket<_TX>` and `Socket<_RX>`.
//!
//! For `_TX`, committing a descriptor means the application has finished writing
//! a packet to the associated UMEM frame. The `commit_` function advances the
//! producer index of the TX ring, signaling to the kernel that the packet is
//! ready to be sent.
//!
//! For `_RX`, committing a descriptor means the application has finished
//! processing a received packet. The `commit_` function returns the UMEM frame
//! to the kernel by placing its descriptor in the Fill Ring, making it available
//! for receiving new packets.
//!
//! ## Main components
//!
//! - `Commit_` trait: Defines the internal `commit_` interface.
//! - `impl Commit_<_TX> for Socket<_TX>`: The implementation of the commit logic
//!   for the transmit socket.
//! - `impl Commit_<_RX> for Socket<_RX>`: The implementation of the commit logic
//!   for the receive socket.

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
    /// Commits a number of descriptors, returning their UMEM frames to the Fill Ring.
    ///
    /// This method should be called after the application has finished processing
    /// the packets in the UMEM frames corresponding to the descriptors. It returns
    /// the frames to the kernel so they can be used to receive new packets.
    ///
    /// # Arguments
    ///
    /// * `count` - The number of descriptors to commit. This must not exceed the
    ///   number of available frames to be read.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns `RingError::NotAvailable` if `count` is greater than the number of
    /// packets available to be read.
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
