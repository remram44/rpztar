use flate2::read::GzDecoder;
use tar::{Archive, Entry, EntryType};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Component, Path};

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

    let tar_gz = fs::File::open(filename)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);

    let destination = Path::new("dst");

    // Delay directory entries until the end
    let mut directories = Vec::new();

    // Unpack entries (similar to Archive::_unpack())
    for entry in archive.entries()? {
        let entry = entry?;
        if entry.header().entry_type() == EntryType::Directory {
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
    mut entry: Entry<'a, R>,
    dst: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    // This extends Entry::unpack_in()

    let mut file_dst = dst.to_path_buf();
    {
        let path = entry.path().map_err(|_| {
            format!("invalid path in entry header: {}", String::from_utf8_lossy(&entry.path_bytes()))
        })?;
        // Check first component is "DATA"
        let mut found_prefix = false;

        for part in path.components() {
            match part {
                // Leading '/' characters, root paths, and '.'
                // components are just ignored and treated as "empty
                // components"
                Component::Prefix(..) | Component::RootDir | Component::CurDir => continue,

                // If any part of the filename is '..', then skip over
                // unpacking the file to prevent directory traversal
                // security issues.  See, e.g.: CVE-2001-1267,
                // CVE-2002-0399, CVE-2005-1918, CVE-2007-4131
                Component::ParentDir => return Ok(false),

                Component::Normal(part) => {
                    if !found_prefix {
                        if part != "DATA".as_ref() as &OsStr {
                            return Err(format!("invalid prefix: {}",  String::from_utf8_lossy(&entry.path_bytes())).into());
                        }
                        found_prefix = true;
                    } else {
                        file_dst.push(part);
                    }
                }
            }
        }
    }

    // Skip cases where only slashes or '.' parts were seen, because
    // this is effectively an empty filename.
    if *dst == *file_dst {
        return Ok(true);
    }

    // Skip entries without a parent (i.e. outside of FS root)
    let parent = match file_dst.parent() {
        Some(p) => p,
        None => return Ok(false),
    };

    if parent.symlink_metadata().is_err() {
        fs::create_dir_all(&parent).map_err(|_| {
            format!("failed to create `{}`", parent.display())
        })?;
    }

    entry.unpack(&file_dst)
        .map_err(|_| format!("failed to unpack `{}`", file_dst.display()))?;

    Ok(true)
}
