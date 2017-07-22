// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Unix-specific extensions to primitives in the `std::fs` module.

#![stable(feature = "rust1", since = "1.0.0")]

use fs::{self, Permissions, OpenOptions};
use io;
use libc;
use path::Path;
use sys_common::{FromInner, AsInner, AsInnerMut};
use sys::platform::fs::MetadataExt as UnixMetadataExt;
use sys::fs::Fs;
use pal::fs::*;

/// Unix-specific extensions to [`File`].
///
/// [`File`]: ../../../../std/fs/struct.File.html
#[stable(feature = "file_offset", since = "1.15.0")]
pub trait FileExt {
    /// Reads a number of bytes starting from a given offset.
    ///
    /// Returns the number of bytes read.
    ///
    /// The offset is relative to the start of the file and thus independent
    /// from the current cursor.
    ///
    /// The current file cursor is not affected by this function.
    ///
    /// Note that similar to [`File::read`], it is not an error to return with a
    /// short read.
    ///
    /// [`File::read`]: ../../../../std/fs/struct.File.html#method.read
    ///
    /// # Examples
    ///
    /// ```
    /// use std::os::unix::prelude::FileExt;
    /// use std::fs::File;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let mut buf = [0u8; 8];
    /// let file = File::open("foo.txt")?;
    ///
    /// // We now read 8 bytes from the offset 10.
    /// let num_bytes_read = file.read_at(&mut buf, 10)?;
    /// println!("read {} bytes: {:?}", num_bytes_read, buf);
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "file_offset", since = "1.15.0")]
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize>;

    /// Writes a number of bytes starting from a given offset.
    ///
    /// Returns the number of bytes written.
    ///
    /// The offset is relative to the start of the file and thus independent
    /// from the current cursor.
    ///
    /// The current file cursor is not affected by this function.
    ///
    /// When writing beyond the end of the file, the file is appropriately
    /// extended and the intermediate bytes are initialized with the value 0.
    ///
    /// Note that similar to [`File::write`], it is not an error to return a
    /// short write.
    ///
    /// [`File::write`]: ../../../../std/fs/struct.File.html#write.v
    ///
    /// # Examples
    ///
    /// ```
    /// use std::os::unix::prelude::FileExt;
    /// use std::fs::File;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let file = File::open("foo.txt")?;
    ///
    /// // We now write at the offset 10.
    /// file.write_at(b"sushi", 10)?;
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "file_offset", since = "1.15.0")]
    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<usize>;
}

#[stable(feature = "file_offset", since = "1.15.0")]
impl FileExt for fs::File {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        self.as_inner().read_at(buf, offset)
    }
    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<usize> {
        self.as_inner().write_at(buf, offset)
    }
}

/// Unix-specific extensions to `Permissions`
#[stable(feature = "fs_ext", since = "1.1.0")]
pub trait PermissionsExt {
    /// Returns the underlying raw `st_mode` bits that contain the standard
    /// Unix permissions for this file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs::File;
    /// use std::os::unix::fs::PermissionsExt;
    ///
    /// # fn run() -> std::io::Result<()> {
    /// let f = File::create("foo.txt")?;
    /// let metadata = f.metadata()?;
    /// let permissions = metadata.permissions();
    ///
    /// println!("permissions: {}", permissions.mode());
    /// # Ok(()) }
    /// ```
    #[stable(feature = "fs_ext", since = "1.1.0")]
    fn mode(&self) -> u32;

    /// Sets the underlying raw bits for this set of permissions.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs::File;
    /// use std::os::unix::fs::PermissionsExt;
    ///
    /// # fn run() -> std::io::Result<()> {
    /// let f = File::create("foo.txt")?;
    /// let metadata = f.metadata()?;
    /// let mut permissions = metadata.permissions();
    ///
    /// permissions.set_mode(0o644); // Read/write for owner and read for others.
    /// assert_eq!(permissions.mode(), 0o644);
    /// # Ok(()) }
    /// ```
    #[stable(feature = "fs_ext", since = "1.1.0")]
    fn set_mode(&mut self, mode: u32);

    /// Creates a new instance of `Permissions` from the given set of Unix
    /// permission bits.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs::Permissions;
    /// use std::os::unix::fs::PermissionsExt;
    ///
    /// // Read/write for owner and read for others.
    /// let permissions = Permissions::from_mode(0o644);
    /// assert_eq!(permissions.mode(), 0o644);
    /// ```
    #[stable(feature = "fs_ext", since = "1.1.0")]
    fn from_mode(mode: u32) -> Self;
}

#[stable(feature = "fs_ext", since = "1.1.0")]
impl PermissionsExt for Permissions {
    fn mode(&self) -> u32 {
        self.as_inner().mode()
    }

    fn set_mode(&mut self, mode: u32) {
        *self = Permissions::from_inner(FromInner::from_inner(mode));
    }

    fn from_mode(mode: u32) -> Permissions {
        Permissions::from_inner(FromInner::from_inner(mode))
    }
}

/// Unix-specific extensions to `OpenOptions`
#[stable(feature = "fs_ext", since = "1.1.0")]
pub trait OpenOptionsExt {
    /// Sets the mode bits that a new file will be created with.
    ///
    /// If a new file is created as part of a `File::open_opts` call then this
    /// specified `mode` will be used as the permission bits for the new file.
    /// If no `mode` is set, the default of `0o666` will be used.
    /// The operating system masks out bits with the systems `umask`, to produce
    /// the final permissions.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs::OpenOptions;
    /// use std::os::unix::fs::OpenOptionsExt;
    ///
    /// # fn main() {
    /// let mut options = OpenOptions::new();
    /// options.mode(0o644); // Give read/write for owner and read for others.
    /// let file = options.open("foo.txt");
    /// # }
    /// ```
    #[stable(feature = "fs_ext", since = "1.1.0")]
    fn mode(&mut self, mode: u32) -> &mut Self;

    /// Pass custom flags to the `flags` argument of `open`.
    ///
    /// The bits that define the access mode are masked out with `O_ACCMODE`, to
    /// ensure they do not interfere with the access mode set by Rusts options.
    ///
    /// Custom flags can only set flags, not remove flags set by Rusts options.
    /// This options overwrites any previously set custom flags.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # #![feature(libc)]
    /// extern crate libc;
    /// use std::fs::OpenOptions;
    /// use std::os::unix::fs::OpenOptionsExt;
    ///
    /// # fn main() {
    /// let mut options = OpenOptions::new();
    /// options.write(true);
    /// if cfg!(unix) {
    ///     options.custom_flags(libc::O_NOFOLLOW);
    /// }
    /// let file = options.open("foo.txt");
    /// # }
    /// ```
    #[stable(feature = "open_options_ext", since = "1.10.0")]
    fn custom_flags(&mut self, flags: i32) -> &mut Self;
}

#[stable(feature = "fs_ext", since = "1.1.0")]
impl OpenOptionsExt for OpenOptions {
    fn mode(&mut self, mode: u32) -> &mut OpenOptions {
        self.as_inner_mut().mode(mode); self
    }

    fn custom_flags(&mut self, flags: i32) -> &mut OpenOptions {
        self.as_inner_mut().custom_flags(flags); self
    }
}

// Hm, why are there casts here to the returned type, shouldn't the types always
// be the same? Right you are! Turns out, however, on android at least the types
// in the raw `stat` structure are not the same as the types being returned. Who
// knew!
//
// As a result to make sure this compiles for all platforms we do the manual
// casts and rely on manual lowering to `stat` if the raw type is desired.
#[stable(feature = "metadata_ext", since = "1.1.0")]
pub trait MetadataExt {
    /// Returns the ID of the device containing the file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let dev_id = meta.dev();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn dev(&self) -> u64;
    /// Returns the inode number.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let inode = meta.ino();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn ino(&self) -> u64;
    /// Returns the rights applied to this file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let mode = meta.mode();
    /// let user_has_write_access      = mode & 0o200;
    /// let user_has_read_write_access = mode & 0o600;
    /// let group_has_read_access      = mode & 0o040;
    /// let others_have_exec_access    = mode & 0o001;
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn mode(&self) -> u32;
    /// Returns the number of hard links pointing to this file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let nb_hard_links = meta.nlink();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn nlink(&self) -> u64;
    /// Returns the user ID of the owner of this file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let user_id = meta.uid();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn uid(&self) -> u32;
    /// Returns the group ID of the owner of this file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let group_id = meta.gid();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn gid(&self) -> u32;
    /// Returns the device ID of this file (if it is a special one).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let device_id = meta.rdev();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn rdev(&self) -> u64;
    /// Returns the total size of this file in bytes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let file_size = meta.size();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn size(&self) -> u64;
    /// Returns the time of the last access to the file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let last_access_time = meta.atime();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn atime(&self) -> i64;
    /// Returns the time of the last access to the file in nanoseconds.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let nano_last_access_time = meta.atime_nsec();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn atime_nsec(&self) -> i64;
    /// Returns the time of the last modification of the file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let last_modification_time = meta.mtime();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn mtime(&self) -> i64;
    /// Returns the time of the last modification of the file in nanoseconds.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let nano_last_modification_time = meta.mtime_nsec();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn mtime_nsec(&self) -> i64;
    /// Returns the time of the last status change of the file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let last_status_change_time = meta.ctime();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn ctime(&self) -> i64;
    /// Returns the time of the last status change of the file in nanoseconds.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let nano_last_status_change_time = meta.ctime_nsec();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn ctime_nsec(&self) -> i64;
    /// Returns the blocksize for filesystem I/O.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let blocksize = meta.blksize();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn blksize(&self) -> u64;
    /// Returns the number of blocks allocated to the file, in 512-byte units.
    ///
    /// Please note that this may be smaller than `st_size / 512` when the file has holes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    /// use std::os::unix::fs::MetadataExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("some_file")?;
    /// let blocks = meta.blocks();
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "metadata_ext", since = "1.1.0")]
    fn blocks(&self) -> u64;
}

#[stable(feature = "metadata_ext", since = "1.1.0")]
impl MetadataExt for fs::Metadata {
    fn dev(&self) -> u64 { self.st_dev() }
    fn ino(&self) -> u64 { self.st_ino() }
    fn mode(&self) -> u32 { self.st_mode() }
    fn nlink(&self) -> u64 { self.st_nlink() }
    fn uid(&self) -> u32 { self.st_uid() }
    fn gid(&self) -> u32 { self.st_gid() }
    fn rdev(&self) -> u64 { self.st_rdev() }
    fn size(&self) -> u64 { self.st_size() }
    fn atime(&self) -> i64 { self.st_atime() }
    fn atime_nsec(&self) -> i64 { self.st_atime_nsec() }
    fn mtime(&self) -> i64 { self.st_mtime() }
    fn mtime_nsec(&self) -> i64 { self.st_mtime_nsec() }
    fn ctime(&self) -> i64 { self.st_ctime() }
    fn ctime_nsec(&self) -> i64 { self.st_ctime_nsec() }
    fn blksize(&self) -> u64 { self.st_blksize() }
    fn blocks(&self) -> u64 { self.st_blocks() }
}

/// Add support for special unix types (block/char device, fifo and socket).
#[stable(feature = "file_type_ext", since = "1.5.0")]
pub trait FileTypeExt {
    /// Returns whether this file type is a block device.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs;
    /// use std::os::unix::fs::FileTypeExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("block_device_file")?;
    /// let file_type = meta.file_type();
    /// assert!(file_type.is_block_device());
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "file_type_ext", since = "1.5.0")]
    fn is_block_device(&self) -> bool;
    /// Returns whether this file type is a char device.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs;
    /// use std::os::unix::fs::FileTypeExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("char_device_file")?;
    /// let file_type = meta.file_type();
    /// assert!(file_type.is_char_device());
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "file_type_ext", since = "1.5.0")]
    fn is_char_device(&self) -> bool;
    /// Returns whether this file type is a fifo.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs;
    /// use std::os::unix::fs::FileTypeExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("fifo_file")?;
    /// let file_type = meta.file_type();
    /// assert!(file_type.is_fifo());
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "file_type_ext", since = "1.5.0")]
    fn is_fifo(&self) -> bool;
    /// Returns whether this file type is a socket.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs;
    /// use std::os::unix::fs::FileTypeExt;
    ///
    /// # use std::io;
    /// # fn f() -> io::Result<()> {
    /// let meta = fs::metadata("unix.socket")?;
    /// let file_type = meta.file_type();
    /// assert!(file_type.is_socket());
    /// # Ok(())
    /// # }
    /// ```
    #[stable(feature = "file_type_ext", since = "1.5.0")]
    fn is_socket(&self) -> bool;
}

#[stable(feature = "file_type_ext", since = "1.5.0")]
impl FileTypeExt for fs::FileType {
    fn is_block_device(&self) -> bool { self.as_inner().is(libc::S_IFBLK) }
    fn is_char_device(&self) -> bool { self.as_inner().is(libc::S_IFCHR) }
    fn is_fifo(&self) -> bool { self.as_inner().is(libc::S_IFIFO) }
    fn is_socket(&self) -> bool { self.as_inner().is(libc::S_IFSOCK) }
}

/// Unix-specific extension methods for [`fs::DirEntry`].
///
/// [`fs::DirEntry`]: ../../../../std/fs/struct.DirEntry.html
#[stable(feature = "dir_entry_ext", since = "1.1.0")]
pub trait DirEntryExt {
    /// Returns the underlying `d_ino` field in the contained `dirent`
    /// structure.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs;
    /// use std::os::unix::fs::DirEntryExt;
    ///
    /// if let Ok(entries) = fs::read_dir(".") {
    ///     for entry in entries {
    ///         if let Ok(entry) = entry {
    ///             // Here, `entry` is a `DirEntry`.
    ///             println!("{:?}: {}", entry.file_name(), entry.ino());
    ///         }
    ///     }
    /// }
    /// ```
    #[stable(feature = "dir_entry_ext", since = "1.1.0")]
    fn ino(&self) -> u64;
}

#[stable(feature = "dir_entry_ext", since = "1.1.0")]
impl DirEntryExt for fs::DirEntry {
    fn ino(&self) -> u64 { self.as_inner().ino() }
}

/// Creates a new symbolic link on the filesystem.
///
/// The `dst` path will be a symbolic link pointing to the `src` path.
///
/// # Note
///
/// On Windows, you must specify whether a symbolic link points to a file
/// or directory.  Use `os::windows::fs::symlink_file` to create a
/// symbolic link to a file, or `os::windows::fs::symlink_dir` to create a
/// symbolic link to a directory.  Additionally, the process must have
/// `SeCreateSymbolicLinkPrivilege` in order to be able to create a
/// symbolic link.
///
/// # Examples
///
/// ```
/// use std::os::unix::fs;
///
/// # fn foo() -> std::io::Result<()> {
/// fs::symlink("a.txt", "b.txt")?;
/// # Ok(())
/// # }
/// ```
#[stable(feature = "symlink", since = "1.1.0")]
pub fn symlink<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> io::Result<()>
{
    Fs::soft_link(src.as_ref(), dst.as_ref())
}

#[stable(feature = "dir_builder", since = "1.6.0")]
/// An extension trait for [`fs::DirBuilder`] for unix-specific options.
///
/// [`fs::DirBuilder`]: ../../../../std/fs/struct.DirBuilder.html
pub trait DirBuilderExt {
    /// Sets the mode to create new directories with. This option defaults to
    /// 0o777.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs::DirBuilder;
    /// use std::os::unix::fs::DirBuilderExt;
    ///
    /// let mut builder = DirBuilder::new();
    /// builder.mode(0o755);
    /// ```
    #[stable(feature = "dir_builder", since = "1.6.0")]
    fn mode(&mut self, mode: u32) -> &mut Self;
}

#[stable(feature = "dir_builder", since = "1.6.0")]
impl DirBuilderExt for fs::DirBuilder {
    fn mode(&mut self, mode: u32) -> &mut fs::DirBuilder {
        self.as_inner_mut().set_mode(mode);
        self
    }
}
