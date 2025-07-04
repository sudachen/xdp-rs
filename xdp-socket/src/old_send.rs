//
// send.rs - AF_XDP Transmit Path Abstractions
//
// Purpose:
//   This module provides abstractions for transmitting (sending) packets through AF_XDP sockets
//   in user space. It manages the ring logic for high-performance, zero-copy packet transmission.
//
// How it works:
//   - Implements the Transmitter type for safe, efficient packet sending using the AF_XDP Tx ring.
//   - Handles ring buffer index management, chunk recycling, and wakeup signaling to the kernel.
//   - Provides error handling for ring full and I/O conditions.
//
// Main components:
//   - Transmitter: Main struct for managing the transmit path of an AF_XDP socket.
//   - TransmitError: Enum for error conditions during transmission.
//

use crate::ring::XdpDesc;
use crate::socket::{AfXdpSocket, Direction};
use std::io;

pub struct Transmitter<'a>(&'a mut AfXdpSocket);

impl AfXdpSocket {
    pub fn tx(&mut self) -> Result<Transmitter<'_>, io::Error> {
        if self.direction == Direction::Rx {
            return Err(io::Error::other("Cannot send on a receive-only socket"));
        }
        Ok(Transmitter(self))
    }
}

/*
  The Tx ring is a circular buffer that holds indexes of chunks to be sent.
  TX| .. consumer .<=. producer .<=. tail .. | >>>
  there are two AF_XDP indexes: consumer and producer; and one additional index - tail.
  The tail points to last available chunk we can use to send packet.
  So all chunks indexed by |producer .. tail| are available to send data.
  if producer is equal to tail, it means that ring is full, and we cannot send data.

  The start configuration is
  TX| consumer = producer .<=. tail | >>>
  what means that ring is empty, and we can put data to the chunk pointed by producer+1.

  when send function needs to put data it uses next logic:
  1. if producer is equal to tail, we need to update tail first.
     to do it, function must get indexes of completed chunks from the completion ring
     then update tx_ring and tail.
  2. if producer is still equal to tail, ring is full, and we cannot send data.
  3. Otherwise, we can put data to the chunk pointed by producer+1 then update producer.

  ATTENTION: TX|tail is stored in socket's tx_tail field.

  Completing ring is a circular buffer that holds indexes of completed chunks.
  C|.. consumer .>. producer .. |
  The start configuration is:
  C|consumer=producer .. |
*/
impl Transmitter<'_> {
    pub fn seek(&mut self) -> Result<u32, TransmitError> {
        let tx_head = self.0.tx_ring.producer();
        if self.0.tx_tail == tx_head {
            // updating tx_ring.head
            let c_ring = &mut self.0.c_ring;
            let c_tail = c_ring.producer();
            let mut c_head = c_ring.consumer();
            if c_tail == c_head {
                // No completed chunks, cannot send data
                self.0
                    .wakeup(false, Direction::Tx)
                    .map_err(TransmitError::Io)?;
                return Err(TransmitError::RingFull);
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
                self.0.tx_ring.increment(&mut self.0.tx_tail);
                *self.0.tx_ring.mut_desc_at(self.0.tx_tail) = desc;
            }
        }
        // !INVARIANT!
        // debug_assert!(tx_head == self.0.tx_ring.producer());
        // debug_assert!(tx_head != self.0.tx_tail);
        let mut tx_head = tx_head; // self.0.tx_ring.producer()
        self.0.tx_ring.increment(&mut tx_head);
        if tx_head == self.0.tx_ring.consumer() {
            return Err(TransmitError::RingFull);
        }
        Ok(tx_head)
    }

    pub fn peek_(&mut self, tx_head: u32, len: usize) -> Result<(&mut [u8], u32), TransmitError> {
        let buf = self.0.tx_ring.mut_bytes_at(&mut self.0.umem, tx_head, len);
        Ok((buf, tx_head))
    }
    pub fn peek(&mut self, len: usize) -> Result<(&mut [u8], u32), TransmitError> {
        let tx_head = self.seek()?;
        self.peek_(tx_head, len)
    }
    pub fn peek_and_kick(&mut self, len: usize) -> Result<(&mut [u8], u32), TransmitError> {
        let head = match self.seek() {
            Ok(head) => head,
            Err(TransmitError::RingFull) => {
                self.0
                    .wakeup(true, Direction::Tx)
                    .map_err(TransmitError::Io)?;
                self.seek()?
            }
            Err(e) => return Err(e),
        };
        self.peek_(head, len)
    }

    pub fn commit(&mut self, tx_head: u32) -> Result<(), TransmitError> {
        let tx_ring = &mut self.0.tx_ring;
        let mut producer = tx_ring.producer();
        if self.0.tx_tail == producer {
            return Err(TransmitError::RingFull);
        }
        tx_ring.increment(&mut producer);
        if producer != tx_head {
            return Err(TransmitError::InvalidTxHead);
        }
        tx_ring.update_producer(tx_head);
        Ok(())
    }

    pub fn kick(&self) -> Result<(), TransmitError> {
        self.0
            .wakeup(true, Direction::Tx)
            .map_err(TransmitError::Io)
    }

    pub fn send(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), TransmitError> {
        let hdr_len = header.map_or(0, |h| h.len());
        let buf_len = data.len() + hdr_len;
        let (buf, tx_head) = self.peek(buf_len)?;
        if let Some(bs) = header {
            buf[0..hdr_len].copy_from_slice(bs);
        }
        buf[hdr_len..].copy_from_slice(data);
        self.commit(tx_head)
    }

    pub fn send_and_kick(
        &mut self,
        data: &[u8],
        header: Option<&[u8]>,
    ) -> Result<(), TransmitError> {
        self.send(data, header).and_then(|_| self.kick())
    }
}

#[derive(Debug)]
pub enum TransmitError {
    RingFull,
    InvalidTxHead,
    Io(io::Error),
}
