//! # Descriptor Seeking in XDP Rings
//!
//! ## Purpose
//!
//! This file implements the logic for finding the next available descriptor in a ring
//! for a new operation. For sending (TX), this means finding an empty slot in the TX
//! ring to place a new packet descriptor.
//!
//! ## How it works
//!
//! It implements the `Seek_` trait for `Socket<_TX>`. The `seek_` method first checks
//! if there are pre-allocated, available frames in the TX ring. If not, it checks the
//! Completion Ring for descriptors of packets that the kernel has finished sending.
//! It reclaims these completed descriptors, making their associated UMEM frames
//! available for new transmissions, and then returns the index of the next free TX slot.
//! The implementation for RX is currently a placeholder.
//!
//! ## Main components
//!
//! - `Seek_` trait: Defines the internal `seek_` interface.
//! - `impl Seek_<_TX> for Socket<_TX>`: The implementation of the seek logic for the
//!   transmit socket, which involves managing available frames and reclaiming completed ones.

use crate::ring::XdpDesc;
use crate::socket::{RingError, Socket, Seek_, _TX, _RX};

/// Implements the seeking logic for a transmit (`TX`) socket.
impl Seek_<_TX> for Socket<_TX> {
    /// Finds the next available descriptor in the TX ring for sending a packet.
    ///
    /// # How it works
    ///
    /// 1.  It first checks if there are any locally cached available descriptors. If so,
    ///     it returns the index of the next one.
    /// 2.  If not, it checks the Completion Ring to see if the kernel has finished
    ///     transmitting any packets.
    /// 3.  If the Completion Ring has descriptors, it reclaims one, moves its
    ///     corresponding UMEM frame address to the TX ring, and returns the index
    ///     of that new descriptor in the TX ring.
    /// 4.  If the Completion Ring is empty, it means the TX ring is full and no
    ///     frames can be reclaimed. In this case it returns a `RingFull` error.
    fn seek_(&mut self) -> Result<u32, RingError> {
        if self.available != 0 {
            let x_head = self.producer & self.x_ring.mod_mask;
            return Ok(x_head);
        }
        let c_ring = &mut self.u_ring;
        let c_producer = c_ring.producer();
        if c_producer == (self.consumer & c_ring.mod_mask) {
            Err(RingError::RingFull)
        } else {
            let c_head = self.consumer & c_ring.mod_mask;
            let addr = c_ring.desc_at(c_head);
            let desc = XdpDesc::new(addr, 0, 0);
            self.consumer += 1;
            c_ring.update_consumer(self.consumer);
            let x_head = self.producer & self.x_ring.mod_mask;
            *self.x_ring.mut_desc_at(x_head) = desc;
            self.available += 1;
            Ok(x_head)
        }
    }
}

/// Implements the seeking logic for a receive (`RX`) socket.
impl Seek_<_RX> for Socket<_RX> {
    /// Finds the next available descriptor in the RX ring for receiving a packet.
    ///
    /// This is currently a placeholder and will be implemented in the future.
    fn seek_(&mut self) -> Result<u32, RingError> {
        todo!()
    }
}
