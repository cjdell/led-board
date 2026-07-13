use crate::{
    make_static,
    tasks::WifiControl,
    types::{EthernetSignal, EthernetSignalMessage, SharedWatchdog},
};
use alloc::vec;
use alloc::vec::Vec;
use core::net::Ipv4Addr;
use cyw43::{PowerManagementMode, aligned_bytes};
use cyw43_pio::PioSpi;
use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_futures::yield_now;
use embassy_net::{ConfigV4, Ipv4Cidr, StackResources};
use embassy_rp::{
    Peri,
    gpio::{Level, Output},
    peripherals::{PIN_23, PIO0, TRNG, USB},
    trng::Trng,
    usb,
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use embassy_usb::{
    class::cdc_acm::{self, BufferedReceiver, CdcAcmClass, Sender},
    driver::EndpointError,
};
use embedded_io_async::Read;
use static_cell::StaticCell;
use utils::sleep;

const RAW_FLASH_BASE: u32 = 0x1c000000; // See XIP_NOCACHE_NOALLOC_NOTRANSLATE_BASE
const WIFI_FIRMWARE_FLASH_OFFSET: u32 = 0x00200000;
const WIFI_FIRMWARE_LENGTH: usize = 231077; // Size of `firmware/blobs/43439A0.bin`

// fifo_full_threshold (RX)
pub const READ_BUF_SIZE: usize = 64;

pub async fn init_ppp(
    spawner: Spawner,
    spi: PioSpi<'static, PIO0, 0>,
    pwr: Peri<'static, PIN_23>,
    driver: usb::Driver<'static, USB>,
    mut trng: Trng<'static, TRNG>,
    ethernet_signal: &'static EthernetSignal,
    watchdog: SharedWatchdog,
) -> (embassy_net::Stack<'static>, WifiControl) {
    // Generate random seed
    let mut seed = [0; 8];
    trng.fill_bytes(&mut seed).await;
    let seed = u64::from_le_bytes(seed);

    // ======== WiFi stuff (just for LED)

    let firmware_address = RAW_FLASH_BASE + WIFI_FIRMWARE_FLASH_OFFSET;

    info!("Loading WiFi firmware from: 0x{:x}", firmware_address);

    let fw = unsafe { core::slice::from_raw_parts(firmware_address as *const u8, WIFI_FIRMWARE_LENGTH) };
    let fw = unsafe { &*(fw as *const [u8] as *const cyw43::Aligned<cyw43::A4, [u8]>) };
    let nvram = aligned_bytes!("../../blobs/nvram_rp2040.bin");
    let clm = aligned_bytes!("../../blobs/43439A0_clm.bin");

    let pwr_pin = Output::new(pwr, Level::Low);

    Timer::after(Duration::from_millis(1_000)).await;

    info!("SPI configured");

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());

    let (_wifi_device, mut control, cyw43_runner) = cyw43::new(state, pwr_pin, spi, fw, nvram).await;

    spawner.spawn(cyw43_task(cyw43_runner).unwrap());

    info!("WiFi driver task started");

    control.init(clm).await;

    control.set_power_management(PowerManagementMode::None).await;

    let led_channel = make_static!(Channel<NoopRawMutex, bool, 1>, Channel::<NoopRawMutex, bool, 1>::new());
    let public_control = WifiControl::new(led_channel.sender());

    // ======== USB stuff

    // Create embassy-usb Config
    let config = {
        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("Embassy");
        config.product = Some("USB-serial example");
        config.serial_number = Some("12345678");
        config.max_power = 100;
        config.max_packet_size_0 = 64;
        config
    };

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut builder = {
        static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

        let builder = embassy_usb::Builder::new(
            driver,
            config,
            CONFIG_DESCRIPTOR.init([0; 256]),
            BOS_DESCRIPTOR.init([0; 256]),
            &mut [], // no msos descriptors
            CONTROL_BUF.init([0; 64]),
        );
        builder
    };

    // Create classes on the builder.
    let mut class = {
        static STATE: StaticCell<cdc_acm::State> = StaticCell::new();
        let state = STATE.init(cdc_acm::State::new());
        CdcAcmClass::new(&mut builder, state, 64)
    };

    // Build the builder.
    let usb = builder.build();

    // ========

    // Run the USB device.
    spawner.spawn(usb_task(usb).unwrap());

    let max_packet_size = class.max_packet_size() as usize;

    let (tx, rx) = class.split();

    let mut buf = make_static!(Vec<u8>, vec![0u8; max_packet_size]); // Must be >= max_packet_size

    // Open serial port
    let port = SerialPort::new(tx, rx.into_buffered(buf));

    // Init network device
    static PPP_STATE: StaticCell<embassy_net_ppp::State<4, 4>> = StaticCell::new();
    let ppp_state = PPP_STATE.init(embassy_net_ppp::State::<4, 4>::new());
    let (ppp_device, ppp_runner) = embassy_net_ppp::new(ppp_state);

    // Init network stack
    static RESOURCES: StaticCell<StackResources<8>> = StaticCell::new();
    let (stack, net_runner) = embassy_net::new(
        ppp_device,
        embassy_net::Config::default(), // don't configure IP yet
        RESOURCES.init(StackResources::new()),
        seed,
    );

    // Launch network task
    spawner.spawn(net_task(net_runner).unwrap());

    // info!("Starting PPP in 5 secs...");

    // sleep(5_000).await;

    // Launch PPP task
    spawner.spawn(ppp_task(spawner, ethernet_signal, stack, ppp_runner, port, watchdog).unwrap());

    info!("PPP task started");

    (stack, public_control)
}

type MyUsbDriver = usb::Driver<'static, USB>;
type MyUsbDevice = embassy_usb::UsbDevice<'static, MyUsbDriver>;

#[embassy_executor::task]
async fn usb_task(mut usb: MyUsbDevice) -> ! {
    usb.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, embassy_net_ppp::Device<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::task(pool_size = 4)]
async fn cyw43_task(
    runner: cyw43::Runner<'static, cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>, cyw43::Cyw43439>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
pub async fn ppp_task(
    spawner: Spawner,
    signal: &'static EthernetSignal,
    stack: embassy_net::Stack<'static>,
    mut runner: embassy_net_ppp::Runner<'static>,
    mut port: SerialPort,
    watchdog: SharedWatchdog,
) -> ! {
    let sender = signal.sender();

    loop {
        let config = embassy_net_ppp::Config {
            username: b"myuser",
            password: b"mypass",
        };

        port.tx.wait_connection().await;

        while !port.tx.dtr() {
            info!("PPP waiting... DTR:{} RTS:{}", port.tx.dtr(), port.tx.rts());

            watchdog.write().await.feed(Duration::from_secs(15));

            sleep(1_000).await;
        }

        info!("PPP running... DTR:{} RTS:{}", port.tx.dtr(), port.tx.rts());

        let Err(err) = runner
            .run(&mut port, config, |ipv4| {
                let Some(addr) = ipv4.address else {
                    warn!("PPP did not provide an IP address.");
                    return;
                };
                info!("Got IP: {:?}", addr);
                let mut dns_servers = heapless::Vec::<Ipv4Addr, 3>::new();
                for s in ipv4.dns_servers.iter().flatten() {
                    let _ = dns_servers.push(*s);
                }
                let config = ConfigV4::Static(embassy_net::StaticConfigV4 {
                    address: Ipv4Cidr::new(addr, 0),
                    gateway: None,
                    dns_servers,
                });
                stack.set_config_v4(config);

                sender.send(EthernetSignalMessage::Connected(addr));
            })
            .await;

        error!("PPP Runner Error: {:?}", err);

        sleep(5_000).await;
    }

    unreachable!()
}

const MAX_BUFFER_SIZE: usize = 1024;

pub struct SerialPort {
    // port: &'static mut CdcAcmClass<'static, Driver<'static, USB>>,
    tx: Sender<'static, usb::Driver<'static, USB>>,
    rx: BufferedReceiver<'static, usb::Driver<'static, USB>>,

    // Internal buffer for BufRead implementation
    buffer: [u8; MAX_BUFFER_SIZE],
    // Range of valid data in buffer [start..end)
    start: usize,
    end: usize,
}

impl SerialPort {
    pub fn new(
        tx: Sender<'static, usb::Driver<'static, USB>>,
        rx: BufferedReceiver<'static, usb::Driver<'static, USB>>,
    ) -> Self {
        Self {
            tx,
            rx,
            buffer: [0u8; MAX_BUFFER_SIZE],
            start: 0,
            end: 0,
        }
    }

    /// Check if internal buffer has data available
    fn buffer_has_data(&self) -> bool {
        self.start < self.end
    }

    /// Get remaining space in buffer
    fn remaining_space(&self) -> usize {
        self.buffer.len() - self.end
    }

    /// Compact buffer by moving unconsumed data to the beginning
    fn compact_buffer(&mut self) {
        if self.start > 0 {
            let len = self.end - self.start;
            self.buffer.copy_within(self.start..self.end, 0);
            self.start = 0;
            self.end = len;
        }
    }
}

impl embedded_io_async::ErrorType for SerialPort {
    type Error = EndpointError;
}

impl embedded_io_async::Read for SerialPort {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.rx.wait_connection().await;

        // info!("READ: {}", buf.len());

        match self.rx.read(&mut *buf).await {
            Ok(n) => {
                return Ok(n);
            }
            Err(err) => {
                info!("Read error: {:?}", defmt::Display2Format(&err));
                return Err(EndpointError::BufferOverflow);
            }
        }
    }
}

impl embedded_io_async::BufRead for SerialPort {
    async fn fill_buf(&mut self) -> Result<&[u8], Self::Error> {
        self.rx.wait_connection().await;

        // info!("FILL BUF: start = {} end = {}", self.start, self.end);

        // If we have data in buffer, return it
        if self.buffer_has_data() {
            return Ok(&self.buffer[self.start..self.end]);
        }

        // Buffer is empty, need to read more data
        // First, compact the buffer if needed
        if self.remaining_space() < READ_BUF_SIZE {
            self.compact_buffer();
        }

        // If still no space after compacting, we have a problem
        if self.remaining_space() == 0 {
            // This shouldn't happen with reasonable buffer sizes
            // but we need to handle it gracefully
            self.start = 0;
            self.end = 0;
        }

        // Read new data into the buffer

        // info!("RTS before: {}", self.rx.rts());

        let mut bytes_read = 0;

        loop {
            match self.rx.read(&mut self.buffer[self.end..]).await {
                Ok(n) => {
                    bytes_read = n;
                    self.end += bytes_read;
                }
                Err(err) => {
                    info!("Read error: {:?}", defmt::Display2Format(&err));
                    return Err(EndpointError::BufferOverflow);
                }
            }

            if bytes_read == 0 {
                // info!("No bytes, waiting...");
                // sleep(100).await;
                yield_now().await;
            } else {
                break;
            }
        }

        // info!("RTS after: {}", self.rx.rts());

        // info!("Read: {}, total buffered: {}", bytes_read, self.end - self.start);

        // Return the available data
        Ok(&self.buffer[self.start..self.end])
    }

    fn consume(&mut self, amt: usize) {
        // info!("CONSUME: {}", amt);

        // Advance the start pointer, but don't go beyond end
        let consumable = (self.end - self.start).min(amt);
        self.start += consumable;

        // If we've consumed everything, reset pointers to beginning
        // This keeps the buffer compact for future reads
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
        }
    }
}

impl embedded_io_async::Write for SerialPort {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.tx.wait_connection().await;

        // info!("WRITE: {}", buf.len());

        // For Write trait, we need to return the error type from tx operations
        // However, UartTx might have a different error type than RxError
        // You might need to adjust the ErrorType implementation or handle conversion

        // Assuming UartTx::write returns Result<usize, SomeError>
        // and you need to convert or handle the error type mismatch

        // For now, let's assume it works or you'll need to adapt:

        match self.tx.write(&buf).await {
            Ok(n) => {
                return Ok(n);
            }
            Err(err) => {
                info!("Write error: {}", defmt::Display2Format(&err));
                return Err(EndpointError::BufferOverflow);
            }
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
