// Observe logs with: defmt-print -e target/thumbv8m.main-none-eabihf/release/YOUR_ELF tcp --host YOUR_IP

// Add the follow to your program...

//  #[embassy_executor::main]
//  async fn main(spawner: Spawner) {
//      ...
//      spawner.spawn(defmt_runner(stack).unwrap());
//      ...
//  }
//
//  #[embassy_executor::task]
//  async fn defmt_runner(stack: Stack<'static>) {
//      TCP_SENDER.runner(stack).await;
//  }

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use defmt::Encoder;
use embassy_net::{Stack, tcp::TcpSocket};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, rwlock::RwLock};
use embassy_time::Duration;
use embedded_io_async::Write as _;

const RTT_PORT: u16 = 19021;

#[defmt::global_logger]
struct TcpLogger;

unsafe impl defmt::Logger for TcpLogger {
    fn acquire() {
        TCP_SENDER.acquire();
    }

    unsafe fn flush() {
        TCP_SENDER.flush();
    }

    unsafe fn release() {
        TCP_SENDER.release();
    }

    unsafe fn write(bytes: &[u8]) {
        TCP_SENDER.write(bytes);
    }
}

pub static TCP_SENDER: TcpSender = TcpSender::new();

pub struct TcpSender {
    channel: Channel<CriticalSectionRawMutex, Vec<u8>, 2000>,
    encoder: RwLock<CriticalSectionRawMutex, Encoder>,
}

impl TcpSender {
    const fn new() -> Self {
        TcpSender {
            channel: Channel::new(),
            encoder: RwLock::new(Encoder::new()),
        }
    }

    fn acquire(&self) {
        let mut encoder = match self.encoder.try_write() {
            Ok(encoder) => encoder,
            Err(_) => return,
        };

        encoder.start_frame(|bytes| {
            self.channel.try_send(bytes.to_vec()).ok();
        });
    }

    fn flush(&self) {}

    fn release(&self) {
        let mut encoder = match self.encoder.try_write() {
            Ok(encoder) => encoder,
            Err(_) => return,
        };

        encoder.end_frame(|bytes| {
            self.channel.try_send(bytes.to_vec()).ok();
        });
    }

    fn write(&self, bytes: &[u8]) {
        let mut encoder = match self.encoder.try_write() {
            Ok(encoder) => encoder,
            Err(_) => return,
        };

        encoder.write(bytes, |bytes| {
            self.channel.try_send(bytes.to_vec()).ok();
        });
    }

    pub async fn runner(&self, stack: Stack<'static>) {
        let mut rx_buffer = [0; 4096];
        let mut tx_buffer = [0; 4096];

        loop {
            let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
            socket.set_timeout(Some(Duration::from_secs(10)));

            if let Err(_) = socket.accept(RTT_PORT).await {
                continue;
            }

            loop {
                let buf = self.channel.receive().await;

                match socket.write_all(&buf).await {
                    Ok(()) => (),
                    Err(_) => break,
                };
            }
        }
    }
}
