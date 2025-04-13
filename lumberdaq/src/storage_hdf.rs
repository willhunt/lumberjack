use crate::Result;
use crate::storage_csv::read_results;
use std::str::FromStr;
use hdf5;
use hdf5::types::VarLenUnicode;
// use ndarray::{ Array, Array2, ArrayView1, Axis };

pub fn create_hdf_file(path: &std::path::PathBuf) -> Result<hdf5::File> {
    let file = hdf5::File::create(path)?;
    return Ok(file);
}

pub fn add_hdf_str_attribute(location: &hdf5::Location, name: &str, value: &str) -> Result<()> {
    let attr = location.new_attr::<VarLenUnicode>().shape(()).create(name)?;
    let attr_value = VarLenUnicode::from_str(value)?;
    attr.as_writer().write_scalar(&attr_value)?;
    return Ok(());
}

pub fn add_hdf_device(file: &hdf5::File, name: &str) -> Result<hdf5::Group> {
    let group = file.create_group(name)?;
    return Ok(group);
}

/// Setup and add new channel to HDF5 file
/// Found useful information at this [repo](https://github.com/pywr/pywr-next/blob/030a7e9e5642e0da7b5d39db262e0ee032ca39ed/pywr-core/src/recorders/hdf.rs#L180)
pub fn add_hdf_channel(device_group: &hdf5::Group, name: &str, units: &str, description: &str, timestamps: Vec<f64>, values: Vec<f64>) -> Result<()> {
    if timestamps.len() != values.len() {
        return Err("Timestamps and values must have the same length".into());
    }

    // let timestamps_view = ArrayView1::from(&timestamps);
    // let values_view = ArrayView1::from(&values);
    // let stacked_data: Array2<f64> = ndarray::stack(Axis(1), &[timestamps_view, values_view])?;
    // let combined_data = stacked_data.as_standard_layout().to_owned();
    // let ds_data = device_group
    //     .new_dataset::<f64>()
    //     .shape(combined_data.shape())
    //     .create(name)?;
    // ds_data.write(&combined_data)?;
    // Add attributes
    // add_hdf_str_attribute(&ds_data, "description", description)?;
    // add_hdf_str_attribute(&ds_data, "timestamp units", "UNIX timestamp")?;
    // add_hdf_str_attribute(&ds_data, "value units", units)?;

    let group = device_group.create_group(name)?;
    add_hdf_str_attribute(&group, "description", description)?;
    add_hdf_str_attribute(&group, "axes", "timestamps")?;
    add_hdf_str_attribute(&group, "signal", "values")?;
    add_hdf_str_attribute(&group, "NX_class", "NXdata")?;
    
    let ds_timestamps = group.new_dataset::<i64>().shape(timestamps.len()).create("timestamps")?;
    add_hdf_str_attribute(&ds_timestamps, "units", "UNIX timestamp")?;
    ds_timestamps.write(&timestamps)?;
    
    let ds_values = group.new_dataset::<f64>().shape(values.len()).create("values")?;
    add_hdf_str_attribute(&ds_values, "units", units)?;
    ds_values.write(&values)?;

    return  Ok(());
}

pub fn convert_results_to_hdf5(csv_path: &std::path::PathBuf) -> Result<()> {
    let json_path = &csv_path.clone().with_extension("json");
    let daq = read_results(csv_path, json_path)?;
    let file = create_hdf_file(&csv_path.clone().with_extension("hdf"))?;
    for device in daq.devices.iter() {
        let group = add_hdf_device(&file, &device.info.name)?;
        for channel in device.channels.iter() {
            let (datetimes, values) = channel.datapoints_as_vectors()?;
            let timestamps  = datetimes.iter()
                .map(|datetime| datetime.timestamp_millis() as f64)
                .collect();
            add_hdf_channel(&group, &channel.info.name, &channel.info.unit, &channel.info.description, timestamps, values)?;
        }
    }

    return Ok(());
}