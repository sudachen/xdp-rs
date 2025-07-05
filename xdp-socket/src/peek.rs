use crate::socket::{Socket, _TX, _RX, Peek_, RingError};
impl Peek_<_TX> for Socket<_TX> {
    fn peek_(&mut self, index: usize, len: usize) -> Result<&mut [u8], RingError> {
        #[cfg(not(feature="no_safety_checks"))]
        if index >= self.available as usize {
            return Err(RingError::InvalidIndex);
        }
        #[cfg(not(feature="no_safety_checks"))]
        if len > self.x_ring.frame_size() as usize {
            return Err(RingError::InvalidLength);
        }
        let x_head = self.producer.wrapping_add(index as u32) & self.x_ring.mod_mask;
        self.x_ring.mut_desc_at(x_head).len = len as u32;
        Ok(self.x_ring.mut_bytes_at(self.frames, x_head, len))
    }
}

impl Peek_<_RX> for Socket<_RX> {
    fn peek_(&mut self, index: usize, _len: usize) -> Result<&mut [u8], RingError> {
        #[cfg(not(feature="no_safety_checks"))]
        if index >= self.available as usize {
            return Err(RingError::InvalidIndex);
        }
        let x_head = self.consumer.wrapping_add(index as u32) & self.x_ring.mod_mask;
        let len = self.x_ring.desc_at(x_head).len as usize;
        Ok(self.x_ring.mut_bytes_at(self.frames, x_head, len))
    }
}
