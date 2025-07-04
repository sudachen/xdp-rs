use crate::socket::{RingError, Socket, _TX, _RX};

impl Socket<_TX>  {
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
