use lumberdaq::daq::Daq;
use lumberdaq::hardware::{ mock_hardware, serial_stream };
use lumberdaq::project::Project;
use lumberdaq::storage_csv::CsvSink;
use lumberdaq::Result;
use std::{thread, time};

fn main() -> Result<()> {
    // Everything this run reads or writes lives under here.
    let project = Project::create("examples/simulated_devices")?;

    println!("Lets create some devices");
    // Mock device
    let mut mock_hardware =
        mock_hardware::create_device("Test device".to_string(), "-".to_string())?;
    mock_hardware::add_channel_random(&mut mock_hardware, "Random 1".to_string())?;

    // Serial stream device
    let mut serial_hardware = serial_stream::create_device(
        "Serial test device".to_string(),
        "Device streaming over serial.".to_string(),
        "COM3".to_string(),
        115200,
    )?;
    serial_stream::add_channel(
        &mut serial_hardware,
        "Pressure".to_string(),
        "Differential pressure sensor".to_string(),
        1,
        "Pa".to_string(),
    )?;
    serial_stream::add_channel(
        &mut serial_hardware,
        "Pump Activation".to_string(),
        "Pump on (1) or off (0)".to_string(),
        3,
        "-".to_string(),
    )?;

    // TODO: Pico Technology / LabJack backends go here.

    // Serial device
    // let serial_port = serialport::SerialPortInfo {
    //     port_name: "COMx".to_string(),
    //     port_type: serialport::SerialPortType::Unknown
    // };

    let mut daq = Daq::new(
        "Example measurement".to_string(),
        "Joesephine Bloggs".to_string(),
        vec![mock_hardware, serial_hardware],
    )?;

    // Setup storage. Swapping csv for another format is a change to this line
    // only; nothing downstream of here knows the format.
    let sink = CsvSink::new(&project.results_path(), &project.header_path())?;
    daq.set_sink(Box::new(sink))
        .unwrap_or_else(|err| panic!("Error intitialising storage: {err}"));
    // Connect to devices. Every device is tried, so one bad port does not hide
    // the state of the rest.
    let report = daq.connect();
    if !report.all_connected() {
        eprintln!();
        eprintln!("Could not connect to {} of {} devices:", report.failed.len(), daq.devices.len());
        for (name, reason) in report.failed.iter() {
            eprintln!("    {}: {}", name, reason);
        }
        // Refusing to start is the safe default for a test rig: recording a run
        // that is quietly missing a device wastes the run. Change this to a
        // warning if you would rather start anyway and let the retry pick it up.
        return Err("Not all devices connected. Fix the above, or remove those devices from the setup.".into());
    }

    for _ in 0..10 {
        // Read devices. A device that drops out is retried in the background
        // and does not stop the others recording.
        for (name, problem) in daq.read() {
            eprintln!("    ! {}: {}", name, problem);
        }
        for device in daq.devices.iter() {
            device.print_latest();
        }
        // Drain what was read into storage, then make it durable. Flushing once
        // per cycle rather than once per datapoint is what keeps the syscall
        // count sane at higher sample rates.
        daq.write()?;
        daq.flush()?;
        // Wait - just for testing, sample rates to be implemeted later.
        thread::sleep(time::Duration::from_millis(200));
    }
    // TODO: Convert results here if required.
    project.write_config(&daq.config())?;
    Ok(())
}
