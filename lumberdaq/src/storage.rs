use crate::Result;
use crate::channel::ChannelInfo;
use crate::config::DaqConfig;
use crate::daq::DaqInfo;
use crate::datapoint::DataPoint;
use crate::device::DeviceInfo;
use serde::{ Deserialize, Serialize };

/// Storage is split into two halves.
///
/// The *header* describes the test: which devices, which channels, what units.
/// It is written once, when recording starts, and is the same regardless of
/// which format the data itself ends up in.
///
/// The *batches* are the data, streamed in as it is acquired. A batch is the
/// datapoints for one channel of one device. Laying those out on disk (long
/// rows, wide rows, columns) is the sink's business, not the caller's.

#[derive(Serialize, Deserialize)]
pub struct DeviceHeader {
    pub info: DeviceInfo,
    pub channels: Vec<ChannelInfo>,
}

#[derive(Serialize, Deserialize)]
pub struct DaqHeader {
    pub info: DaqInfo,
    pub devices: Vec<DeviceHeader>,
}

impl DaqHeader {
    /// Flatten a setup into the shape the csv sidecar wants: names and units,
    /// with the hardware details left out.
    pub fn from_config(config: &DaqConfig) -> DaqHeader {
        DaqHeader {
            info: config.info.clone(),
            devices: config.devices.iter().map(|device|
                DeviceHeader {
                    info: device.info.clone(),
                    channels: device.hardware.channel_infos(),
                }
            ).chain(config.calculated.iter().map(|calculated|
                // Calculated channels are a device as far as results are
                // concerned: they have names, units and values over time.
                DeviceHeader {
                    info: calculated.info.clone(),
                    channels: calculated.channels.iter().map(|c| c.info.clone()).collect(),
                }
            )).collect(),
        }
    }
}

/// The datapoints acquired for a single channel, on its way to storage.
pub struct Batch {
    pub device: String,
    pub channel: String,
    pub datapoints: Vec<DataPoint>,
}

/// Somewhere acquired data can be written to.
///
/// Implementors decide their own on-disk layout and their own metadata story,
/// so swapping csv for parquet or sqlite means adding a type here rather than
/// changing anything that produces data.
pub trait DataSink {
    /// Called once before any data, to record what is being measured.
    ///
    /// This takes the whole setup rather than a flattened header. Names and
    /// units are enough to label a column, but not enough to say whether two
    /// runs measured the same thing: that needs the port, the baud rate and
    /// which field of the frame each channel reads. A sink that only wants the
    /// labels can call `DaqHeader::from_config`.
    fn init(&mut self, config: &DaqConfig) -> Result<()>;

    /// Write one channel's worth of acquired data.
    ///
    /// This does not guarantee the data has reached disk. Call `flush` for that.
    fn write_batch(&mut self, batch: &Batch) -> Result<()>;

    /// Push everything buffered so far out to disk.
    fn flush(&mut self) -> Result<()>;
}
