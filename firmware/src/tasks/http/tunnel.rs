//! A [`picoserve::io::Socket`] backed by a UART stream slot relayed from the
//! ESP32-C3 slave radio (see [`crate::tasks::radio`]).
//!
//! The C3 terminates TCP and pipes the bytes over the link; this adapter
//! presents them to picoserve as an ordinary socket so the whole web app —
//! HTTP, WebSockets, OTA — works with no IP stack on the Pico.

use alloc::vec::Vec;

use embedded_io_async::{ErrorType, Read, ReadReady, Write};
use picoserve::{EmbassyRuntime, NoGracefulShutdown, Timeouts, io::Socket, time::Timer};

use crate::tasks::radio::{self, StreamChannel, StreamChunk, TxSlot};

/// Socket IO error (the underlying link is CRC-guarded and self-healing, so
/// failures are effectively "connection gone").
#[derive(Debug)]
pub struct TunnelError;

impl core::fmt::Display for TunnelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("tunnel link error")
    }
}

impl embedded_io::Error for TunnelError {
    fn kind(&self) -> embedded_io::ErrorKind {
        embedded_io::ErrorKind::Other
    }
}

impl core::error::Error for TunnelError {}

/// Read half: drains the per-stream queue handed over by the radio task.
/// Every frame fully handed up is acknowledge-acked (once per frame, when
/// picoserve has consumed the whole chunk) so the radio's upload window
/// (request bodies, OTA) keeps opening as picoserve consumes.
pub struct TunnelReader {
    id: u16,
    queue: &'static StreamChannel,
    pending: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl ErrorType for TunnelReader {
    type Error = TunnelError;
}

impl Read for TunnelReader {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        loop {
            if self.pos < self.pending.len() {
                let n = core::cmp::min(buf.len(), self.pending.len() - self.pos);
                buf[..n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
                self.pos += n;
                // Release the buffer once fully consumed to keep memory flat,
                // and ack the frame exactly then: the window counts frames, so
                // partial reads of one chunk must not ack more than once.
                if self.pos == self.pending.len() {
                    self.pending.clear();
                    self.pos = 0;
                    radio::ack_stream(self.id).await;
                }
                return Ok(n);
            }

            if self.eof {
                return Ok(0);
            }

            match self.queue.receive().await {
                StreamChunk::Data(bytes) => {
                    if bytes.is_empty() {
                        // Never sent by a correct peer, but ack-and-skip so a
                        // stray empty frame can't silently hold a window slot.
                        radio::ack_stream(self.id).await;
                        continue;
                    }
                    self.pending = bytes;
                    self.pos = 0;
                }
                StreamChunk::Eof => {
                    self.eof = true;
                    return Ok(0);
                }
            }
        }
    }
}

impl ReadReady for TunnelReader {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        if self.pos < self.pending.len() || self.eof {
            return Ok(true);
        }

        match self.queue.try_receive() {
            Ok(StreamChunk::Data(bytes)) => {
                self.pending = bytes;
                self.pos = 0;
                Ok(true)
            }
            Ok(StreamChunk::Eof) => {
                self.eof = true;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }
}

/// Write half: chunks output into `STREAM_DATA` frames toward the C3,
/// gated by the flow-control window. An error means the stream is gone
/// (torn down by teardown, corruption, or a dead peer) — picoserve then
/// ends the request and the client retries.
pub struct TunnelWriter {
    id: u16,
    tx: &'static TxSlot,
}

impl ErrorType for TunnelWriter {
    type Error = TunnelError;
}

impl Write for TunnelWriter {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if radio::send_stream_data(self.id, self.tx, buf).await {
            Ok(buf.len())
        } else {
            Err(TunnelError)
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// A picoserve socket relayed over the radio link.
pub struct TunnelSocket {
    id: u16,
    queue: &'static StreamChannel,
    tx: &'static TxSlot,
}

impl TunnelSocket {
    pub fn new(id: u16, queue: &'static StreamChannel, tx: &'static TxSlot) -> Self {
        Self { id, queue, tx }
    }

    pub fn id(&self) -> u16 {
        self.id
    }
}

impl Socket<EmbassyRuntime> for TunnelSocket {
    type Error = TunnelError;

    type ReadHalf<'a> = TunnelReader;

    type WriteHalf<'a> = TunnelWriter;

    fn split(&mut self) -> (Self::ReadHalf<'_>, Self::WriteHalf<'_>) {
        (
            TunnelReader {
                id: self.id,
                queue: self.queue,
                pending: Vec::new(),
                pos: 0,
                eof: false,
            },
            TunnelWriter {
                id: self.id,
                tx: self.tx,
            },
        )
    }

    async fn abort<T: Timer<EmbassyRuntime>>(
        self,
        _timeouts: &Timeouts,
        _timer: &mut T,
    ) -> Result<(), picoserve::Error<Self::Error>> {
        radio::send_stream_close(self.id).await;
        Ok(())
    }

    async fn shutdown<T: Timer<EmbassyRuntime>>(
        self,
        _timeouts: &Timeouts,
        _timer: &mut T,
    ) -> Result<(), picoserve::Error<Self::Error>> {
        radio::send_stream_close(self.id).await;
        Ok(())
    }
}

/// Marker used for the disconnection info of tunnel connections.
pub type TunnelDisconnection = picoserve::DisconnectionInfo<NoGracefulShutdown>;
