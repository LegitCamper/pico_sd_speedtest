#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::spi::Spi;
use embassy_rp::{gpio, spi};
use embassy_time::{Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::asynchronous::{SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use gpio::{Level, Output};
use {defmt_rtt as _, panic_probe as _};

// size of chunk to read & write
const BUFF_SIZE: usize = 2048;

const FILE_NAME: &str = "RpSdST";

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut config = spi::Config::default();
    config.frequency = 400_000;
    let spi = Spi::new(
        p.SPI0,
        p.PIN_2,
        p.PIN_3,
        p.PIN_4,
        p.DMA_CH0,
        p.DMA_CH1,
        spi::Config::default(),
    );
    let cs = Output::new(p.PIN_5, Level::High);

    let device = ExclusiveDevice::new(spi, cs, embassy_time::Delay).unwrap();
    let sdcard = SdCard::new(device, embassy_time::Delay);

    // Now that the card is initialized, the SPI clock can go faster
    let mut config = spi::Config::default();
    config.frequency = 16_000_000;
    sdcard.spi(|dev| dev.bus_mut().set_config(&config));

    let volume_mgr = VolumeManager::new(sdcard, DummyTimeSource {});

    // Wait for sdcard
    let volume = {
        info!("Waiting for sd card...");
        loop {
            if let Ok(vol) = volume_mgr.open_volume(VolumeIdx(0)).await {
                break vol;
            }
            warn!("Could not init Sd card");
            Timer::after_millis(50).await;
        }
    };

    let root = volume.open_root_dir().unwrap();

    let mut buf = [0; BUFF_SIZE];

    info!("Starting Tests");

    const TEST_COUNT: usize = 1000;
    const TOTAL_BYTES: usize = TEST_COUNT * BUFF_SIZE;

    loop {
        info!("Running Speed test on file {}", FILE_NAME);
        let _ = root.delete_file_in_dir(FILE_NAME).await;
        let file = unwrap!(
            root.open_file_in_dir(
                FILE_NAME,
                embedded_sdmmc::asynchronous::Mode::ReadWriteCreate,
            )
            .await
        );

        // write test
        let start = Instant::now();
        for _ in 0..TEST_COUNT {
            buf.fill(0xCC);
            unwrap!(file.write(&buf).await);
        }
        info!(
            "Finished write test: {}/s",
            TOTAL_BYTES as f32 / Instant::now().duration_since(start).as_millis() as f32
        );

        // read test
        let start = Instant::now();
        for _ in 0..TEST_COUNT {
            unwrap!(file.read(&mut buf).await);
        }
        info!(
            "Finished read test: {}/s",
            TOTAL_BYTES as f32 / Instant::now().duration_since(start).as_millis() as f32
        );

        info!("Closing file");
        unwrap!(file.close().await);

        Timer::after_secs(2).await;
    }
}

pub struct DummyTimeSource {}
impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp::from_calendar(2022, 1, 1, 0, 0, 0).unwrap()
    }
}
