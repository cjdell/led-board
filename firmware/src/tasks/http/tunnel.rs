//! A [`picoserve::io::Socket`] backed by a UART stream slot relayed from the
//! ESP32-C3 slave radio (see [`crate::tasks::radio`]).
//!
//! The C3 terminates TCP and pipes the bytes over the link; this adapter
//! presents them to picoserve as an ordinary socket so the whole web app —
//! HTTP, WebSockets, OTA — works with no IP stack on the Pico.

use alloc::vec::Vec;

use embedded_io_async::{ErrorType, Read, ReadReady, Write};
use picoserve::{EmbassyRuntime, NoGracefulShutdown, Timeouts, io::Socket, time::Timer};

use crate::tasks::radio::{self, StreamChannel, StreamChunk};

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
pub struct TunnelReader {
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
                // Release the buffer once fully consumed to keep memory flat.
                if self.pos == self.pending.len() {
                    self.pending.clear();
                    self.pos = 0;
                }
                return Ok(n);
            }

            if self.eof {
                return Ok(0);
            }

            match self.queue.receive().await {
                StreamChunk::Data(bytes) => {
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

/// Write half: chunks output into `STREAM_DATA` frames toward the C3.
pub struct TunnelWriter {
    id: u16,
}

impl ErrorType for TunnelWriter {
    type Error = TunnelError;
}

impl Write for TunnelWriter {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        radio::send_stream_data(self.id, buf).await;
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// A picoserve socket relayed over the radio link.
pub struct TunnelSocket {
    id: u16,
    queue: &'static StreamChannel,
}

impl TunnelSocket {
    pub fn new(id: u16, queue: &'static StreamChannel) -> Self {
        Self { id, queue }
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
                queue: self.queue,
                pending: Vec::new(),
                pos: 0,
                eof: false,
            },
            TunnelWriter { id: self.id },
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
