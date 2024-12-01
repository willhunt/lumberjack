use crate::{device, Result};
use crate::storage_csv::{ read_csv_file, read_json_file };
use std::str::FromStr;
use hdf5;
// use::ndarray::{Array1, ArrayView};

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

pub fn create_hdf_file(path: &std::path::PathBuf) -> Result<hdf5::File> {
    let file = hdf5::File::create(path)?;
    return Ok(file);
}

pub fn add_hdf_str_attribute(location: &hdf5::Location, name: &str, value: &str) -> Result<()> {
    let attr = location.new_attr::<hdf5::types::VarLenUnicode>().shape(()).create(name)?;
    let attr_value =  match hdf5::types::VarLenUnicode::from_str(value) {
        Ok(v) => v,
        Err(e) => panic!("Cannot convert atrribute str to unicode: {e}"),
    };
    // attr.write(&value)?;
    attr.as_writer().write_scalar(&attr_value)?;
    return Ok(());
}

pub fn add_hdf_device(file: &hdf5::File, name: &str) -> Result<hdf5::Group> {
    let group = file.create_group(name)?;
    return Ok(group);
}

/// Setup and add new channel to HDF5 file
/// Found useful information at this [repo](https://github.com/pywr/pywr-next/blob/030a7e9e5642e0da7b5d39db262e0ee032ca39ed/pywr-core/src/recorders/hdf.rs#L180)
pub fn add_hdf_channel(device_group: &hdf5::Group, name: &str, units: &str, description: &str, timestamps: Vec<i64>, values: Vec<f64>) -> Result<hdf5::Group> {
    let group = device_group.create_group(name)?;
    add_hdf_str_attribute(&group, "description", description)?;
    // Add datasets
    // let array_timestamps = Array1::from_vec(timestamps).view();
    let ds_timestamps = group.new_dataset::<i64>().shape(timestamps.len()).create("timestamps")?;
    add_hdf_str_attribute(&ds_timestamps, "units", "UNIX timestamp")?;
    ds_timestamps.write(&timestamps)?;
    
    let ds_values = group.new_dataset::<f64>().shape(values.len()).create("values")?;
    add_hdf_str_attribute(&ds_values, "units", units)?;
    ds_values.write(&values)?;
    return  Ok(group);
}

/// Write HDF5 chunk to file
/// 'ds' used to refernce 'dataset'
pub fn write_channel(channel_group: &hdf5::Group, timestamps: Vec<i64>, values: Vec<f64>) -> Result<()> {
    let len_timestamps = timestamps.len();
    if len_timestamps != values.len() {
        return Err("The timestamp and value slices must be the same length.".into());
    }
    let ds_timestamps = channel_group.dataset("timestamps")?;
    ds_timestamps.write(&timestamps)?;
    let _ds_values = channel_group.dataset("values")?;
    return  Ok(());
}

pub fn convert_results_to_hdf5(path: &std::path::PathBuf) -> Result<()> {
    let header = read_json_file(&path.clone().with_extension("json"))?;
    let records_map = read_csv_file(path)?;
    for (key1, value1) in records_map {
        for (key2, value2) in value1 {
            println!("Key: {}, Value: {}", key1, key2);
        }
    }
    let file = create_hdf_file(&path.clone().with_extension("hdf"))?;
    for device in header.devices.iter() {
        let group = add_hdf_device(&file, &device.info.name)?;
        for channel in device.channels.iter() {
            add_hdf_channel(&group, &channel.name, &channel.unit, &channel.description, vec![0,1,2], vec![0.,2.,1.])?;
        }
    }

    return Ok(());
}