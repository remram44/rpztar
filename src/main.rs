use anyhow::{Context, Result as AResult, anyhow};
use flate2::read::GzDecoder;
use nix::unistd::{FchownatFlags, Gid, Uid, fchownat};
use tar::{Archive, Entry, EntryType};
use std::collections::HashSet;
use std::convert::TryInto;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::os::unix::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};

fn main() -> AResult<()> {
    // Read arguments
    let mut args = env::args();
    args.next().unwrap();
    let tar_filename = match args.next() {
        Some(v) => v,
        None => return Err(anyhow!("Missing argument")),
    };
    let list_filename = args.next();
    match args.next() {
        Some(_) => return Err(anyhow!("Too many arguments")),
        None => {}
    }

    // Read file list
    let files: Option<HashSet<PathBuf>> = match list_filename {
        Some(list_filename) => {
            let list_file = fs::File::open(list_filename)
                .with_context(|| "Error opening list file")?;
            let list_file = std::io::BufReader::new(list_file);
            let mut files = HashSet::new();
            for file in list_file.split(0u8) {
                let file = file.with_context(|| "Error reading list")?;
                if file.len() > 0 {
                    let osstr: OsString = OsStringExt::from_vec(file);
                    files.insert(osstr.into());
                }
            }
            Some(files)
        }
        None => None
    };

    // Open tar file
    let mut tar = fs::File::open(tar_filename)
        .with_context(|| "Error opening tar file")?;

    // Decompress maybe
    let decompressed = GzDecoder::new(&mut tar);
    if decompressed.header().is_some() {
        unpack_rpz(decompressed, files, true)?;
    } else {
        drop(decompressed);
        tar.seek(SeekFrom::Start(0))?;
        unpack_rpz(tar, files, true)?;
    }

    Ok(())
}

fn unpack_rpz<R: Read>(tar: R, files: Option<HashSet<PathBuf>>, recurse: bool) -> AResult<()> {
    let mut archive = Archive::new(tar);

    let destination = Path::new("");

    // Delay directory entries until the end
    let mut directories = Vec::new();

    // Unpack entries (similar to Archive::_unpack())
    for entry in archive.entries()? {
        let entry = entry?;

        let path = entry.path().with_context(|| {
            format!("invalid path in entry header: {}", String::from_utf8_lossy(&entry.path_bytes()))
        })?;

        // Ignore these, they are valid in a RPZ
        if path.starts_with("METADATA") || path.starts_with("EXTENSIONS") {
            continue;
        }

        // If we find a DATA.tar.gz, then we won't find any data, it's in there
        // Recurse into that tar
        if recurse && path == Path::new("DATA.tar.gz") {
            return unpack_rpz(GzDecoder::new(entry), files, false);
        }

        // Check if the file is in our list
        let path = get_canonical_path(Path::new(""), &entry)?;
        let path = match path {
            Some(p) => p,
            None => continue,
        };
        if let Some(ref files) = files {
            if !files.contains(&path) {
                continue;
            }
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

fn get_canonical_path<'a, R: Read>(
    prefix: &Path,
    entry: &Entry<'a, R>,
) -> AResult<Option<PathBuf>> {
    let path = entry.path().with_context(|| {
        format!("invalid path in entry header: {}", String::from_utf8_lossy(&entry.path_bytes()))
    })?;

    let mut file_dst = prefix.to_path_buf();
    {
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
                Component::ParentDir => return Err(anyhow!("invalid path: {:?}", path)),

                Component::Normal(part) => {
                    if !found_prefix {
                        if part != "DATA".as_ref() as &OsStr {
                            return Ok(None);
                        }
                        found_prefix = true;
                    } else {
                        file_dst.push(part);
                    }
                }
            }
        }
    }
    Ok(Some(file_dst))
}

fn unpack<'a, R: Read>(
    mut entry: Entry<'a, R>,
    dst: &Path,
) -> AResult<()> {
    // This extends Entry::unpack_in()

    let file_dst = get_canonical_path(dst, &entry)?;
    let file_dst = match file_dst {
        Some(p) => p,
        None => return Ok(()),
    };

    // Skip cases where only slashes or '.' parts were seen, because
    // this is effectively an empty filename.
    if dst == &file_dst {
        return Ok(());
    }

    // Skip entries without a parent (i.e. outside of FS root)
    let parent = match file_dst.parent() {
        Some(p) => p,
        None => return Ok(()),
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
                            fs::remove_file(&ancestor)
                                .with_context(|| format!("Error deleting {:?} to unpack {:?}", ancestor, file_dst))?;
                        } else {
                            // Exists and is a directory: good, we'll restore
                            // permissions later
                            continue;
                        }
                    }
                    Err(e) => return Err(e).with_context(|| format!("Error stat()ing {:?} to unpack {:?}", ancestor, file_dst)),
                }

                fs::create_dir(&ancestor)
                    .with_context(|| format!("Error creating directory {:?} to unpack {:?}", ancestor, file_dst))?;
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
                    fs::remove_dir_all(&file_dst)
                        .with_context(|| format!("Error removing directory {:?} to extract file over", file_dst))?;
                }
            } else {
                // Is a file: remove
                fs::remove_file(&file_dst)
                    .with_context(|| format!("Error removing file {:?} to extract {:?} over", file_dst, entry.header().entry_type()))?;
            }
        }
        Err(e) => return Err(e).with_context(|| format!("Error deleting {:?} to unpack over it", file_dst)),
    }

    entry.set_preserve_permissions(true);
    entry.set_preserve_mtime(true);
    entry.unpack(&file_dst)
        .with_context(|| format!("failed to unpack `{:?}`", file_dst))?;

    // Restore ownership
    fchownat(
        None,
        &file_dst,
        Some(Uid::from_raw(entry.header().uid()?.try_into()?)),
        Some(Gid::from_raw(entry.header().gid()?.try_into()?)),
        FchownatFlags::NoFollowSymlink,
    ).with_context(|| format!("Error restoring ownership of {:?}", file_dst))?;

    Ok(())
}
