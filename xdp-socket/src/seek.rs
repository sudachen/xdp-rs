//! # Descriptor Seeking in XDP Rings
//!
//! ## Purpose
//!
//! This file implements the logic for advancing the cursor in an XDP ring to find
//! the next available descriptor for a new operation. For sending (TX), this means
//! finding an empty slot in the TX ring to place a new packet descriptor. For
//! receiving (RX), this means finding a descriptor in the RX ring that points to a
//! received packet.
//!
//! ## How it works
//!
//! It implements the `Seek_` trait for both `Socket<_TX>` and `Socket<_RX>`.
//!
//! For `_TX`, the `seek_` method ensures there are enough free descriptors in the TX
//! ring for sending packets. If the ring is low on free descriptors, it checks the
//! Completion Ring for packets that the kernel has finished sending. It reclaims
//! these completed descriptors, making their associated UMEM frames available for new
//! transmissions, and updates the count of available TX slots.
//!
//! For `_RX`, the `seek_` method checks for newly received packets in the RX ring
//! that are ready to be read by the application. It updates its internal count of
//! available packets by checking the ring's producer index, which is advanced by the
//! kernel when packets are received.
//!
//! ## Main components
//!
//! - `Seek_` trait: Defines the internal `seek_` interface.
//! - `impl Seek_<_TX> for Socket<_TX>`: The implementation of the seek logic for the
//!   transmit socket.
//! - `impl Seek_<_RX> for Socket<_RX>`: The implementation of the seek logic for the
//!   receive socket.

use crate::ring::XdpDesc;
use crate::socket::{_RX, _TX, RingError, Seek_, Socket};

/// Implements the seeking logic for a transmit (`TX`) socket.
impl Seek_<_TX> for Socket<_TX> {
    /// Seeks to the next available descriptor in the TX ring by reclaiming completed
    /// packets from the Completion Ring.
    ///
    /// # Arguments
    ///
    /// * `count` - The number of descriptors to seek.
    ///
    /// # Returns
    ///
    /// A `Result` containing the number of descriptors successfully sought, or a
    /// `RingError` if the operation fails.
    fn seek_(&mut self, count: usize) -> Result<usize, RingError> {
        if self.available as usize >= count {
            return Ok(count);
        }
        let c_ring = &mut self.u_ring;
        let c_producer = c_ring.producer();
        if c_producer == self.consumer {
            Err(RingError::RingFull)
        } else {
            loop {
                let c_head = self.consumer & c_ring.mod_mask;
                let addr = c_ring.desc_at(c_head);
                let desc = XdpDesc::new(addr, 0, 0);
                self.consumer = self.consumer.wrapping_add(1);
                c_ring.update_consumer(self.consumer);
                let x_head = self.producer & self.x_ring.mod_mask;
                *self.x_ring.mut_desc_at(x_head) = desc;
                self.available += 1;
                if self.available as usize >= count || c_producer == self.consumer {
                    break;
                }
            }
            Ok(self.available as usize)
        }
    }
}

/// Implements the seeking logic for a receive (`RX`) socket.
impl Seek_<_RX> for Socket<_RX> {
    /// Seeks to the next available descriptor in the RX ring.
    ///
    /// # Arguments
    ///
    /// * `count` - The number of descriptors to seek.
    ///
    /// # Returns
    ///
    /// A `Result` containing the number of descriptors successfully sought, or a
    /// `RingError` if the operation fails.
    fn seek_(&mut self, count: usize) -> Result<usize, RingError> {
        if self.available as usize >= count {
            return Ok(count);
        }
        let x_producer = self.x_ring.producer();
        if x_producer == self.consumer {
            Err(RingError::RingEmpty)
        } else {
            self.available = x_producer.wrapping_sub(self.consumer);
            Ok(self.available.min(count as u32) as usize)
        }
    }
}
