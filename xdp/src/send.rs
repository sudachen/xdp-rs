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

use crate::socket::{AfXdpSocket, Direction};
use std::os::fd::AsRawFd as _;
use std::sync::atomic::Ordering;
use std::{io, ptr};
use crate::mmap::XdpDesc;

pub struct Transmitter<'a>(&'a mut AfXdpSocket);

impl AfXdpSocket {
    pub fn tx(&mut self) -> Result<Transmitter<'_>, io::Error> {
        if self.direction == Direction::Rx {
            return Err(io::Error::other("Cannot send on a receive-only socket"));
        }
        Ok(Transmitter(self))
    }
}

impl Transmitter<'_> {
    /*
      The Tx ring is a circular buffer that holds indexes of chunks to be sent.
      TX| .. consumer .>. producer .>. tail .. |
      there are two AF_XDP indexes: consumer and producer; and one additional index - tail.
      The tail points to last available chunk we can use to send packet.
      So all chunks indexed by |producer .. tail| are available to send data.
      if producer is equal to tail, it means that ring is full, and we cannot send data.

      The start configuration is
      TX| consumer = producer .. tail |
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
    pub fn send(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), TransmitError> {
        let tx_ring = &mut self.0.tx_ring;
        let c_ring = &mut self.0.c_ring;
        let mut tx_head = tx_ring.producer();
        if self.0.tx_tail == tx_head {
            // updating tx_ring.head
            let c_tail = c_ring.producer();
            let mut c_head = c_ring.consumer();
            if c_tail == c_head {
                // No completed chunks, cannot send data
                self.tx_wakeup().map_err(TransmitError::Io)?;
                return Err(TransmitError::RingFull);
            }
            while c_tail != c_head {
                // get completed chunk descriptor from completion ring
                c_ring.increment(&mut c_head);
                let mut desc = XdpDesc { addr: c_ring.desc_at(c_head), len: 0, options: 0 };
                c_ring.update_consumer(c_head);
                // put it back to the tx_ring
                desc.len = 0;
                tx_ring.increment(&mut self.0.tx_tail);
                *tx_ring.mut_desc_at(self.0.tx_tail) = desc;
            }
        }
        // copy data to the available chunk and update producer ptr on tx_ring
        let hdr_len = header.map_or(0, |h| h.len());
        let buf_len = data.len() + hdr_len;
        let buf = tx_ring.mut_bytes_at(&mut self.0.umem, tx_head, buf_len);
        if let Some(bs) = header {
            buf[0..hdr_len].copy_from_slice(bs);
        }
        buf[hdr_len ..].copy_from_slice(data);
        tx_ring.increment(&mut tx_head);
        tx_ring.update_producer(tx_head);
        Ok(())
    }
    pub fn send_and_wakeup(&mut self, data: &[u8], header: Option<&[u8]>) -> Result<(), TransmitError> {
        self.send(data,header)?;
        self.tx_wakeup().map_err(TransmitError::Io)
    }

    pub fn wait_for_completion(&mut self) -> Result<(), TransmitError> {
        loop {
            self.tx_wakeup().map_err(TransmitError::Io)?;
            let c_ring = &mut self.0.c_ring;
            let c_head = c_ring.consumer();
            let c_tail = c_ring.producer();
            if c_tail != c_head { break } // no completed chunks, exit loop
        }
        Ok(())
    }
    pub fn wait_for_transition(&mut self) -> Result<(), TransmitError> {
        loop {
            self.tx_wakeup().map_err(TransmitError::Io)?;
            let tx_ring = &mut self.0.tx_ring;
            let tx_head = tx_ring.consumer();
            let tx_tail = tx_ring.producer();
            if tx_tail == tx_head { break } // no completed chunks, exit loop
        }
        Ok(())
    }

    pub fn tx_wakeup(&self) -> Result<(), io::Error> {
        let need_wakeup = unsafe {
            (*self.0.tx_ring.mmap.flags).load(Ordering::Relaxed) & libc::XDP_RING_NEED_WAKEUP != 0
        };
        if need_wakeup
            && 0 > unsafe {
                libc::sendto(
                    self.0.fd.as_raw_fd(),
                    ptr::null(),
                    0,
                    libc::MSG_DONTWAIT,
                    ptr::null(),
                    0,
                )
            }
        {
            match io::Error::last_os_error().raw_os_error() {
                None | Some(libc::EBUSY | libc::ENOBUFS | libc::EAGAIN) => {}
                Some(libc::ENETDOWN) => {
                    // TODO: better handling
                    log::warn!("network interface is down, cannot wake up");
                }
                Some(e) => {
                    return Err(io::Error::from_raw_os_error(e));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum TransmitError {
    RingFull,
    Io(io::Error),
}

