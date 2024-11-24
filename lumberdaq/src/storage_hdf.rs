use crate::Result;
use std::str::FromStr;
use hdf5;

// let hdf_file = create_hdf_file(&self.hdf_path)?;
// Add devices
// for device in self.devices.iter_mut() {
//     let device_group = add_hdf_device(&hdf_file, &device.name)?;
//     for channel in device.channels.iter_mut() {
//         let channel_group = add_hdf_channel(
//             &device_group,
//             channel.channel_data.name.as_str(),
//             channel.channel_data.unit.as_str(),
//             channel.channel_data.description.as_str()
//         )?;
//         channel.channel_data.hdf5_group = Some(channel_group);
//     }
// }

pub fn create_hdf_file(path: &std::path::PathBuf) -> Result<hdf5::File, hdf5::Error> {
    let file = hdf5::File::create(path)?;
    return Ok(file);
}

pub fn add_hdf_str_attribute(location: &hdf5::Location, name: &str, value: &str) -> Result<(), hdf5::Error> {
    let attr = location.new_attr::<hdf5::types::VarLenUnicode>().shape(()).create(name)?;
    let attr_value =  match hdf5::types::VarLenUnicode::from_str(value) {
        Ok(v) => v,
        Err(e) => panic!("Cannot convert atrribute str to unicode: {e}"),
    };
    // attr.write(&value)?;
    attr.as_writer().write_scalar(&attr_value)?;
    return Ok(());
}

pub fn add_hdf_device(file: &hdf5::File, name: &str) -> Result<hdf5::Group, hdf5::Error> {
    let group = file.create_group(name)?;
    return Ok(group);
}

/// Setup and add new channel to HDF5 file
/// Found useful information at this [repo](https://github.com/pywr/pywr-next/blob/030a7e9e5642e0da7b5d39db262e0ee032ca39ed/pywr-core/src/recorders/hdf.rs#L180)
pub fn add_hdf_channel(device_group: &hdf5::Group, name: &str, units: &str, description: &str) -> Result<hdf5::Group, hdf5::Error> {
    let group = device_group.create_group(name)?;
    add_hdf_str_attribute(&group, "description", description)?;
    // Add datasets
    let ds_timestamps = group.new_dataset::<i64>().create("timestamps")?;
    add_hdf_str_attribute(&ds_timestamps, "units", "UNIX timestamp")?;
    let ds_values = group.new_dataset::<f64>().create("values")?;
    add_hdf_str_attribute(&ds_values, "units", units)?;
    return  Ok(group);
}

/// Write HDF5 chunk to file
/// 'ds' used to refernce 'dataset'
pub fn write_channel_chunk(channel_group: &hdf5::Group, timestamps: Vec<i64>, values: Vec<f64>) -> Result<(), hdf5::Error> {
    let len_chunk = timestamps.len();
    if len_chunk != values.len() {
        return Err(hdf5::Error::Internal("The timestamp and value slices must be the same length.".to_string()));
    }
    let ds_timestamps = channel_group.dataset("timestamps")?;
    ds_timestamps.resize((len_chunk, 1, 1))?;
    // I don't understand the selection arguent here, maybe it is the chunk index??? See what this does
    ds_timestamps.write_slice(&timestamps, (0, .., ..))?;

    let _ds_values = channel_group.dataset("values")?;

    return  Ok(());
}
