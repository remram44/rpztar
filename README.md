custom tar -xf replacement as a static binary

What is this
------------

This is a simple Rust program that extracts a list of file from a tar archive.
It is meant to be used in [ReproZip](https://github.com/VIDA-NYU/reprozip) and
is probably not generally useful.

The program takes as argument the archive and a list of null-byte-delimited
filenames. Additionally it expects that paths in the archive are prefixed by
`DATA/` and ignores paths that aren't. It always extracts to the current
directory (cwd).

Why is this needed?
-------------------

tar behaves the way you expect when extracting files over files (replace) or
directories over directories (merge), but a problem arise when extracting files
over directories. Either it will fail (tar won't be able to remove a directory
that's not empty) or you use `--recursive-unlink` (but then extracting
directories over directories will delete too, and not merge).

This is needed by the [ReproZip](https://github.com/ViDA-NYU/reprozip) project.
The reprounzip component builds virtual machines and unpacks the files needed
to reproduce an experiment. In some cases the base image's layout is
significantly different from the experiment's files: e.g. /lib might be a
directory or a symlink depending on your distribution.

Reprounzip downloads the static binaries from this project's releases page:

    https://github.com/remram44/rpztar/releases/download/v1/rpzsudo-{arch}

where `{arch}` is one of: `i686`, `x86_64` (`uname -m`).
