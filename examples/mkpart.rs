extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate libparted;

use libparted::*;
use std::io;
use std::env;
use std::process::exit;
use std::ptr;
use std::str;

enum Unit {
    Sectors(u64),
    Mebibytes(u64),
    Megabytes(u64),
}

fn get_config<I: Iterator<Item = String>>(mut args: I) -> io::Result<(String, u64, Unit)> {
    fn config_err(msg: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, msg)
    }

    let device = args.next().ok_or_else(|| config_err("no device provided"))?;
    let start_str = args.next().ok_or_else(|| config_err("no start provided"))?;
    let length_str = args.next().ok_or_else(|| config_err("no length provided"))?;
    let start = start_str
        .parse::<u64>()
        .or_else(|_| Err(config_err("invalid start value")))?;

    let length = if length_str.ends_with("MB") {
        length_str[..length_str.len() - 2]
            .parse::<u64>()
            .map(Unit::Megabytes)
    } else if length_str.ends_with("M") {
        length_str[..length_str.len() - 1]
            .parse::<u64>()
            .map(Unit::Mebibytes)
    } else {
        length_str.parse::<u64>().map(Unit::Sectors)
    };

    let length = length.map_err(|_| config_err("invalid sector length"))?;

    Ok((device, start, length))
}

#[derive(Debug, Fail)]
pub enum PartedError {
    #[fail(display = "unable to open device: {}", why)] OpenDevice { why: io::Error },
    #[fail(display = "unable to create new geometry: {}", why)] CreateGeometry { why: io::Error },
    #[fail(display = "unable to create new disk: {}", why)] CreateDisk { why: io::Error },
    #[fail(display = "unable to create new partition: {}", why)] CreatePartition { why: io::Error },
    #[fail(display = "unable to get exact constraint from geometry")] ExactConstraint,
    #[fail(display = "unable to add partition to disk: {}", why)] AddPartition { why: io::Error },
    #[fail(display = "unable to commit changes to disk: {}", why)] CommitChanges { why: io::Error },
    #[fail(display = "invalid file system type")] InvalidFileSystemType,
}

// TODO: Figure out how to create an 'Unformatted' partition.
fn create_partition(device: &str, start: u64, length: Unit) -> Result<(), PartedError> {
    // Get and open the device; then use that to get the geometry and disk from the device.
    let mut dev = Device::new(&device).map_err(|why| PartedError::OpenDevice { why })?;

    // Get the sector length of the new partition.
    let length = match length {
        Unit::Sectors(sectors) => sectors,
        Unit::Mebibytes(m) => m * 1000 * 1000 / dev.sector_size(),
        Unit::Megabytes(mb) => mb * 1024 * 1024 / dev.sector_size(),
    };

    let geometry = Geometry::new(&dev, start as i64, length as i64)
        .map_err(|why| PartedError::CreateGeometry { why })?;
    let mut disk = Disk::new(&mut dev).map_err(|why| PartedError::CreateDisk { why })?;

    // Create an unformatted file system type.
    let fs_type = None;
    let part_type = PartitionType::PED_PARTITION_NORMAL;

    // Create a new partition from the disk, geometry, and the type.
    let mut partition = Partition::new(
        &mut disk,
        part_type,
        fs_type.as_ref(),
        geometry.start(),
        geometry.length(),
    ).map_err(|why| PartedError::CreatePartition { why })?;

    if partition.is_flag_available(PartitionFlag::PED_PARTITION_LBA) {
        let _ = partition.set_flag(PartitionFlag::PED_PARTITION_LBA, true);
    }

    // Also get the exact constraints of the geometry.
    let constraint = geometry.exact().ok_or(PartedError::ExactConstraint)?;

    // Add the partition to the disk, and set the corresponding partition flag.
    if let Err(why) = disk.add_partition(&mut partition, &constraint) {
        return Err(PartedError::AddPartition { why });
    }

    // Commit changes to the disk, and exit the function, which will clean up
    // the constructed objects from libparted automatically.
    if let Err(why) = disk.commit() {
        return Err(PartedError::CommitChanges { why });
    }

    Ok(())
}

fn main() {
    let (device, start, length) = match get_config(env::args().skip(1)) {
        Ok(config) => config,
        Err(why) => {
            eprintln!("mkpart error: {}", why);
            eprintln!("\tUsage: mkpart <device> <start> <length>");
            exit(1);
        }
    };

    match create_partition(&device, start, length) {
        Ok(()) => (),
        Err(why) => {
            eprintln!("mkpart: {} errored: {}", device, why);
            exit(1);
        }
    }
}
