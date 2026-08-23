use lumberdaq::daq::Daq;
use lumberdaq::hardware::{ mock_hardware, serial_stream };
use lumberdaq::project::Project;
use lumberdaq::session::DeviceEvent;
#[allow(unused_imports)]
use lumberdaq::storage_csv::CsvSink;
use lumberdaq::storage_sqlite::SqliteSink;
use lumberdaq::Result;
use std::time;

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

    // Each device reads at its own rate on its own thread, so a slow one no
    // longer holds up a fast one.
    mock_hardware.sample_interval = time::Duration::from_millis(500);
    serial_hardware.sample_interval = time::Duration::from_millis(100);

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

    // Setup storage. Nothing downstream of here knows the format, so choosing
    // one is this line and nothing else.
    //
    //   csv     long format plus a json sidecar. Append only, so a run that
    //           dies halfway still leaves a readable file, and anything can
    //           open it.
    //   sqlite  normalised, so device and channel names are stored once
    //           instead of on every row, the setup lives in the same file, and
    //           reading one channel back is an indexed lookup.
    let sink = SqliteSink::new(&project.database_path())?;
    // let sink = CsvSink::new(&project.results_path(), &project.header_path())?;
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

    // Each device now reads on its own thread at its own rate, so this blocks
    // for the length of the run rather than driving it. Nothing can look at a
    // device while its thread holds it, so anything worth knowing arrives here.
    daq.run_for(time::Duration::from_secs(5), &mut |event| match event {
        DeviceEvent::Problem { device, error } => {
            eprintln!("    ! {}: {}", device, error);
        }
        DeviceEvent::Connected { device } => {
            println!("    {} reconnected", device);
        }
        DeviceEvent::Disconnected { device, cause } => {
            eprintln!(
                "    ! {} disconnected: {}",
                device,
                cause.unwrap_or_else(|| "unknown".to_string())
            );
        }
    })?;

    for device in daq.devices.iter() {
        device.print_latest();
    }
    // TODO: Convert results here if required.
    project.write_config(&daq.config())?;
    Ok(())
}
