use crate::ring::XdpDesc;
use crate::socket::{RingError, Socket, Seek_, _TX, _RX};

impl Seek_<_TX> for Socket<_TX> {
    fn seek_(&mut self) -> Result<u32, RingError> {
        let x_head = self.x_ring.producer();
        if self.tail == x_head {
            // updating x_ring.head
            let c_ring = &mut self.u_ring;
            let c_tail = c_ring.producer();
            let mut c_head = c_ring.consumer();
            if c_tail == c_head {
                // No completed chunks, cannot send data
                self.kick(false)
                    .map_err(RingError::Io)?;
                return Err(RingError::RingFull);
            } else {
                // c_tail != c_head
                c_ring.increment(&mut c_head);
                let mut desc = XdpDesc {
                    addr: c_ring.desc_at(c_head),
                    len: 0,
                    options: 0,
                };
                c_ring.update_consumer(c_head);
                // put it back to the tx_ring
                desc.len = 0;
                self.x_ring.increment(&mut self.tail);
                *self.x_ring.mut_desc_at(self.tail) = desc;
            }
        }
        // !INVARIANT!
        // debug_assert!(x_head == self.x_ring.producer());
        // debug_assert!(x_head != self.tail);
        let mut x_head = x_head; // adding mutability
        self.x_ring.increment(&mut x_head);
        if x_head == self.x_ring.consumer() {
            return Err(RingError::RingFull);
        }
        Ok(x_head)
    }
}

impl Seek_<_RX> for Socket<_RX> {
    fn seek_(&mut self) -> Result<u32, RingError> {
        todo!()
    }
}
