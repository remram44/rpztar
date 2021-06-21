use flate2::read::GzDecoder;
use tar::Archive;
use std::env;
use std::fs::File;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read argument
    let mut args = env::args();
    args.next().unwrap();
    let filename = match args.next() {
        Some(v) => v,
        None => return Err("Missing argument".into()),
    };
    match args.next() {
        Some(_) => return Err("Too many arguments".into()),
        None => {}
    }

    let tar_gz = File::open(filename)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);

    let destination = "dst";

    // Delay directory entries until the end
    let mut directories = Vec::new();

    // Unpack entries (similar to Archive::_unpack())
    for entry in archive.entries()? {
        let entry = entry?;
        if entry.header().entry_type() == tar::EntryType::Directory {
            directories.push(entry);
        } else {
            unpack(entry, destination)?;
        }
    }
    for entry in directories {
        unpack(entry, destination)?;
    }

    Ok(())
}

fn unpack<'a, R: Read>(
    mut entry: tar::Entry<'a, R>,
    destination: &str,
) -> std::io::Result<()> {
    // This extends Entry::unpack_in()
    entry.unpack_in(destination)?;
    Ok(())
}
