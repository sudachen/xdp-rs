use crate::ring::XdpDesc;
use crate::socket::{RingError, Socket, Seek_, _TX, _RX};

impl Seek_<_TX> for Socket<_TX> {
    fn seek_(&mut self) -> Result<u32, RingError> {
        let x_head = self.producer & self.x_ring.mod_mask;
        if self.available != 0 {
            // There are available chunks, so we can send data
            return Ok(x_head);
        }
        let c_ring= &mut self.u_ring;
        let c_producer = c_ring.producer();
        if c_producer  == (self.consumer & c_ring.mod_mask) {
            // No completed chunks, cannot send data
            self.kick(false)
                .map_err(RingError::Io)?;
            Err(RingError::RingFull)
        } else {
            let c_head= self.consumer & c_ring.mod_mask;
            let addr = c_ring.desc_at(c_head); 
            let desc = XdpDesc::new(addr, 0, 0);
            self.consumer += 1;
            c_ring.update_consumer(self.consumer);
            *self.x_ring.mut_desc_at(x_head) = desc;
            self.available += 1;
            Ok(x_head)
        }
    }
}

impl Seek_<_RX> for Socket<_RX> {
    fn seek_(&mut self) -> Result<u32, RingError> {
        todo!()
    }
}
