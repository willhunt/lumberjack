use lumberdaq::daq::Daq;
use lumberdaq::hardware::mock_hardware;
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

    // TODO: Pico Technology / LabJack backends go here.

    // Serial device
    // let serial_port = serialport::SerialPortInfo {
    //     port_name: "COMx".to_string(),
    //     port_type: serialport::SerialPortType::Unknown
    // };

    let mut daq = Daq::new(
        "Example measurement".to_string(),
        "Joesephine Bloggs".to_string(),
        vec![mock_hardware],
    )?;

    // Setup storage. Swapping csv for another format is a change to this line
    // only; nothing downstream of here knows the format.
    let sink = CsvSink::new(&project.results_path(), &project.header_path())?;
    daq.set_sink(Box::new(sink))
        .unwrap_or_else(|err| panic!("Error intitialising storage: {err}"));
    // Connect to devices
    daq.connect()?;

    for _ in 0..10 {
        // Read devices
        for device in daq.devices.iter_mut() {
            device.read()?;
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
