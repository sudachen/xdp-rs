
use crate::socket::{RingError,_TX,Socket};

impl Socket<_TX> {
    pub fn send(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), RingError> {
        let hdr_len = header.map_or(0, |h| h.len());
        let buf_len = data.len() + hdr_len;
        let (buf, tx_head) = self.peek(buf_len)?;
        if let Some(bs) = header {
            buf[0..hdr_len].copy_from_slice(bs);
        }
        buf[hdr_len..].copy_from_slice(data);
        self.commit(tx_head)
    }

    pub fn send_blocking(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), RingError> {
        self.send(data, header)?;
        self.poll_wait(None).map_err(RingError::Io)?;
        Ok(())
    }
}
