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
