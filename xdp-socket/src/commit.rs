use crate::ring::XdpDesc;
use crate::socket::{RingError, Socket, _TX, _RX};

impl Socket<_TX>  {
    pub fn commit(&mut self, x_head: u32) -> Result<(), RingError> {
        let tx_ring = &mut self.x_ring;
        let mut producer = tx_ring.producer();
        if self.tail == producer {
            return Err(RingError::RingFull);
        }
        tx_ring.increment(&mut producer);
        if producer != x_head {
            return Err(RingError::InvalidTxHead);
        }
        tx_ring.update_producer(x_head);
        Ok(())
    }
}
