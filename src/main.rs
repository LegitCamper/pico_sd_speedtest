#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::spi::Spi;
use embassy_rp::{gpio, spi};
use embassy_time::{Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::asynchronous::{
    BlockDevice, Directory, Mode, SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use gpio::{Level, Output};
use {defmt_rtt as _, panic_probe as _};

// size of chunk to read & write
const BUFF_SIZE: usize = 10240;

const LOOP_NUM: usize = 100;

const DIR_NAME: &str = "ST";

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut config = spi::Config::default();
    config.frequency = 400_000;
    let spi = Spi::new(
        p.SPI0, p.PIN_2, p.PIN_3, p.PIN_4, p.DMA_CH0, p.DMA_CH1, config,
    );
    let cs = Output::new(p.PIN_5, Level::High);

    let device = ExclusiveDevice::new(spi, cs, embassy_time::Delay).unwrap();
    let sdcard = SdCard::new(device, embassy_time::Delay);

    // Now that the card is initialized, the SPI clock can go faster
    let mut config = spi::Config::default();
    config.frequency = 32_000_000;
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
    let test_dir = match root.open_dir(DIR_NAME).await {
        Ok(dir) => dir,
        Err(_) => {
            root.make_dir_in_dir(DIR_NAME).await.unwrap();
            root.open_dir(DIR_NAME).await.unwrap()
        }
    };

    loop {
        info!("Running speed tests");

        let start = Instant::now();
        write_test("NonPar", &test_dir).await;
        let (unit, speed) = calculate_throughput(
            (BUFF_SIZE * LOOP_NUM) as f32 / Instant::now().duration_since(start).as_secs() as f32,
        );
        match unit {
            ThroughputUnit::Bytes => info!("Finished write test: {} B/s", speed),
            ThroughputUnit::KB => info!("Finished write test: {} KB/s", speed),
            ThroughputUnit::MB => info!("Finished write test: {} MB/s", speed),
        }

        let start = Instant::now();
        read_test("NonPar", &test_dir).await;
        let (unit, speed) = calculate_throughput(
            (BUFF_SIZE * LOOP_NUM) as f32 / Instant::now().duration_since(start).as_secs() as f32,
        );
        match unit {
            ThroughputUnit::Bytes => info!("Finished read test: {} B/s", speed),
            ThroughputUnit::KB => info!("Finished read test: {} KB/s", speed),
            ThroughputUnit::MB => info!("Finished read test: {} MB/s", speed),
        }
    }

    unwrap!(test_dir.close());
    unwrap!(root.close());

    info!("Done");
}

enum ThroughputUnit {
    Bytes,
    KB, // Kilobytes
    MB, // Megabytes
}

fn calculate_throughput(speed_in_bytes: f32) -> (ThroughputUnit, f32) {
    if speed_in_bytes >= 1_000_000.0 {
        // If the speed is >= 1,000,000 bytes, return MB/s
        let speed_in_mb = speed_in_bytes / (1024.0 * 1024.0); // Convert to MB
        (ThroughputUnit::MB, speed_in_mb)
    } else if speed_in_bytes >= 1000.0 {
        // If the speed is >= 1000 bytes, return KB/s (without needing divisibility by 1000)
        let speed_in_kb = speed_in_bytes / 1024.0; // Convert to KB
        (ThroughputUnit::KB, speed_in_kb)
    } else {
        // Otherwise, return B/s
        (ThroughputUnit::Bytes, speed_in_bytes)
    }
}

async fn write_test<B: BlockDevice>(name: &str, dir: &Directory<'_, B, DummyTimeSource, 4, 4, 1>)
where
    <B as embedded_sdmmc::asynchronous::BlockDevice>::Error: Format,
{
    let buf = [0xCC; BUFF_SIZE];
    let _ = dir.delete_file_in_dir(name).await;
    let file = unwrap!(dir.open_file_in_dir(name, Mode::ReadWriteCreate).await);
    for _ in 0..LOOP_NUM {
        unwrap!(file.write(&buf).await);
    }
    unwrap!(file.close().await);
}

async fn read_test<B: BlockDevice>(name: &str, dir: &Directory<'_, B, DummyTimeSource, 4, 4, 1>)
where
    <B as embedded_sdmmc::asynchronous::BlockDevice>::Error: Format,
{
    let mut buf = [0xCC; BUFF_SIZE];
    let file = unwrap!(dir.open_file_in_dir(name, Mode::ReadOnly).await);
    for _ in 0..LOOP_NUM {
        unwrap!(file.read(&mut buf).await);
    }
    unwrap!(file.close().await);
}

pub struct DummyTimeSource {}
impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp::from_calendar(2022, 1, 1, 0, 0, 0).unwrap()
    }
}
