use flate2::read::GzDecoder;
use nix::unistd::{Gid, Uid, chown};
use tar::{Archive, Entry, EntryType};
use std::collections::HashSet;
use std::convert::TryInto;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, Read};
use std::path::{Component, Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read arguments
    let mut args = env::args();
    args.next().unwrap();
    let tar_filename = match args.next() {
        Some(v) => v,
        None => return Err("Missing argument".into()),
    };
    let list_filename = match args.next() {
        Some(v) => v,
        None => return Err("Missing argument".into()),
    };
    match args.next() {
        Some(_) => return Err("Too many arguments".into()),
        None => {}
    }
    // Read file list
    let files: HashSet<Vec<u8>> = {
        let list_file = fs::File::open(list_filename)?;
        let list_file = std::io::BufReader::new(list_file);
        let mut files = HashSet::new();
        for file in list_file.split(0u8) {
            let file = file?;
            if file.len() > 0 {
                files.insert(file);
            }
        }
        files
    };

    // Open tar
    let tar_gz = fs::File::open(tar_filename)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);

    let destination = Path::new("dst");

    // Delay directory entries until the end
    let mut directories = Vec::new();

    // Unpack entries (similar to Archive::_unpack())
    for entry in archive.entries()? {
        let entry = entry?;

        // Check if the file is in our list
        if !files.contains(entry.path_bytes().as_ref()) {
            continue;
        }

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

    // Create parent directories, removing existing files
    let mut ancestor = dst.to_path_buf();
    for part in parent.components() {
        match part {
            Component::Normal(part) => {
                ancestor.push(part);

                match fs::symlink_metadata(&ancestor) {
                    // Does not exist: good
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Ok(m) => {
                        if !m.is_dir() {
                            // Exists and is a file: remove
                            fs::remove_file(&ancestor)?;
                        }
                        // Exists and is a directory: good, we'll restore
                        // permissions later
                    }
                    Err(e) => return Err(e.into()),
                }

                fs::create_dir(&ancestor)?;
            }
            _ => {}
        }
    }

    // Remove existing file or directory at destination
    match fs::symlink_metadata(&file_dst) {
        // Does not exist: good
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Ok(m) => {
            if m.is_dir() {
                if entry.header().entry_type() == EntryType::Directory {
                    // Is a directory, as expected: ignore
                    // unpack() will restore permissions
                } else {
                    // Is a directory, where we want a file: remove
                    eprintln!("removing directory {:?}", &file_dst);
                    fs::remove_dir_all(&file_dst)?;
                }
            } else {
                // Is a file: remove
                fs::remove_file(&file_dst)?;
            }
        }
        Err(e) => return Err(e.into()),
    }

    entry.unpack(&file_dst)
        .map_err(|_| format!("failed to unpack `{}`", file_dst.display()))?;

    // Restore ownership
    chown(
        &file_dst,
        Some(Uid::from_raw(entry.header().uid()?.try_into()?)),
        Some(Gid::from_raw(entry.header().gid()?.try_into()?)),
    )?;

    Ok(true)
}
