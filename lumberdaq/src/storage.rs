use crate::Result;
use crate::channel::ChannelInfo;
use crate::daq::{ Daq, DaqInfo };
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
    /// Take a metadata snapshot of a running Daq, leaving the Daq untouched.
    pub fn from_daq(daq: &Daq) -> DaqHeader {
        DaqHeader {
            info: daq.info.clone(),
            devices: daq.devices.iter().map(|device|
                DeviceHeader {
                    info: device.info.clone(),
                    channels: device.channels.iter().map(|channel| channel.info.clone()).collect(),
                }
            ).collect(),
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
    fn init(&mut self, header: &DaqHeader) -> Result<()>;

    /// Write one channel's worth of acquired data.
    ///
    /// This does not guarantee the data has reached disk. Call `flush` for that.
    fn write_batch(&mut self, batch: &Batch) -> Result<()>;

    /// Push everything buffered so far out to disk.
    fn flush(&mut self) -> Result<()>;
}
