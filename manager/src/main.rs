use clap::{Parser, Subcommand};

mod ioctl;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new device and connect it
    Connect { addr: String, port: u16 },

    /// Disconnect the device with the specified minor number
    Disconnect { minor: u64 },

    /// Get the server ip and port of the specified device
    GetServer { device: String },

    /// List all connected devices
    List,
}

fn open_ctl_device() -> std::fs::File {
    std::fs::File::open("/dev/tcpuart0").expect("Failed to open control device")
}

fn connect_device(addr: String, port: u16) -> std::io::Result<()> {
    let ctl_device = open_ctl_device();

    let addr = addr
        .parse::<std::net::Ipv4Addr>()
        .expect("Invalid IP address");
    let to = ioctl::ConnectTo {
        addr: u32::from(addr),
        port,
    };

    let minor = match ioctl::connect_to(&ctl_device, to) {
        Ok(minor) => minor,
        Err(ioctl::IoctlError::NoSlotsLeft) => {
            eprintln!("No slots left for new devices");
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "No slots left",
            ));
        }
        Err(ioctl::IoctlError::Other(err)) => {
            eprintln!("Failed to connect device: {err}");
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Ioctl failed",
            ));
        }
        _ => unreachable!(),
    };

    println!("Created device /dev/tcpuart{minor}");
    Ok(())
}

fn disconnect_device(minor: u64) -> std::io::Result<()> {
    let ctl_device = open_ctl_device();

    match ioctl::disconnect(&ctl_device, minor) {
        Ok(()) => {
            println!("Disconnected device /dev/tcpuart{minor}");
            Ok(())
        }
        Err(ioctl::IoctlError::DeviceNotFound) => {
            eprintln!("Device /dev/tcpuart{minor} not found");
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Device not found",
            ))
        }
        Err(ioctl::IoctlError::Other(err)) => {
            eprintln!("Failed to disconnect device: {err}");
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Ioctl failed",
            ))
        }
        _ => unreachable!(),
    }
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Connect { addr, port } => connect_device(addr, port),
        Commands::Disconnect { minor } => disconnect_device(minor),
        Commands::GetServer { device } => {
            println!("Getting server for {device}");
            todo!()
        }
        Commands::List => {
            println!("Listing all connected devices");
            todo!()
        }
    }
}
