use clap::{Parser, Subcommand};
use std::io;
use std::net::Ipv4Addr;

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

    /// Try to destroy the device with the specified minor number
    Destroy { minor: u64 },

    /// Get the server ip and port for the device with the specified minor number
    GetServer { minor: u32 },

    /// List all connected devices
    List,
}

fn open_ctl_device() -> io::Result<std::fs::File> {
    std::fs::File::open("/dev/tcpuart0").map_err(|err| {
        io::Error::other(format!(
            "Failed to open control device /dev/tcpuart0: {err}"
        ))
    })
}

fn connect_error_to_io(err: ioctl::IoctlError) -> io::Error {
    match err {
        ioctl::IoctlError::NoSlotsLeft => io::Error::other("No slots left for new devices"),
        ioctl::IoctlError::Other(err) => {
            io::Error::other(format!("Failed to connect device: {err}"))
        }
        _ => io::Error::other("Unexpected ioctl error"),
    }
}

fn destroy_error_to_io(minor: u64, err: ioctl::IoctlError) -> io::Error {
    match err {
        ioctl::IoctlError::DeviceBusy => io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("Cannot destroy device /dev/tcpuart{minor} because it is busy"),
        ),
        ioctl::IoctlError::DeviceNotFound => io::Error::new(
            io::ErrorKind::NotFound,
            format!("Device /dev/tcpuart{minor} not found"),
        ),
        ioctl::IoctlError::Other(err) => {
            io::Error::other(format!("Failed to destroy device: {err}"))
        }
        _ => io::Error::other("Unexpected ioctl error"),
    }
}

fn get_server_error_to_io(minor: u32, err: ioctl::IoctlError) -> io::Error {
    match err {
        ioctl::IoctlError::DeviceNotFound => io::Error::new(
            io::ErrorKind::NotFound,
            format!("Device /dev/tcpuart{minor} not found"),
        ),
        ioctl::IoctlError::Other(err) => {
            io::Error::other(format!("Failed to get server info: {err}"))
        }
        _ => io::Error::other("Unexpected ioctl error"),
    }
}

fn connect_device(addr: String, port: u16) -> std::io::Result<()> {
    let ctl_device = open_ctl_device()?;

    let addr = addr.parse::<Ipv4Addr>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid IPv4 address: {addr}"),
        )
    })?;
    let to = ioctl::ConnectTo {
        addr: u32::from(addr),
        port,
    };

    let minor = ioctl::connect_to(&ctl_device, to).map_err(connect_error_to_io)?;

    println!("Created device /dev/tcpuart{minor}");
    Ok(())
}

fn destroy_device(minor: u64) -> std::io::Result<()> {
    let ctl_device = open_ctl_device()?;

    ioctl::destroy(&ctl_device, minor).map_err(|err| destroy_error_to_io(minor, err))?;
    println!("Destroyed device /dev/tcpuart{minor}");
    Ok(())
}

fn get_server_info(minor: u32) -> std::io::Result<()> {
    let ctl_device = open_ctl_device()?;

    let info = ioctl::get_server_info(&ctl_device, minor)
        .map_err(|err| get_server_error_to_io(minor, err))?;
    let ip = Ipv4Addr::from(info.addr);
    println!(
        "Device /dev/tcpuart{minor} is connected to {ip}:{}",
        info.port
    );
    Ok(())
}

fn run() -> std::io::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Connect { addr, port } => connect_device(addr, port),
        Commands::Destroy { minor } => destroy_device(minor),
        Commands::GetServer { minor } => get_server_info(minor),
        Commands::List => {
            println!("Listing all connected devices");
            todo!()
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
