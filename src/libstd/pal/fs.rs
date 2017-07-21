// Copyright 2017 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use io;
use cmp::{PartialEq,Eq};
use fmt::Debug;
use path::{Path,PathBuf};
use ffi::OsString;

pub(crate) trait FsPal {
    type File: FilePal<Self>;
    type Metadata: MetadataPal<Self>;
    type FileType: FileTypePal;
    type Permissions: Clone + PartialEq + Eq + Debug;
    type OpenOptions: OpenOptionsPal;
    type DirBuilder: DirBuilderPal;
    type DirEntry: DirEntryPal<Self>;
    type ReadDir: Debug + Iterator<Item=io::Result<Self::DirEntry>>;
    type SystemTime;

    fn remove_file(path: &Path) -> io::Result<()>;
    fn metadata(path: &Path) -> io::Result<Self::Metadata>;
    fn symlink_metadata(path: &Path) -> io::Result<Self::Metadata>;
    fn rename(from: &Path, to: &Path) -> io::Result<()>;
    fn copy(from: &Path, to: &Path) -> io::Result<u64>;
    fn hard_link(src: &Path, dst: &Path) -> io::Result<()>;
    fn soft_link(src: &Path, dst: &Path) -> io::Result<()>;
    fn read_link(path: &Path) -> io::Result<PathBuf>;
    fn canonicalize(path: &Path) -> io::Result<PathBuf>;
    fn remove_dir(path: &Path) -> io::Result<()>;
    fn remove_dir_all(path: &Path) -> io::Result<()>;
    fn read_dir(path: &Path) -> io::Result<Self::ReadDir>;
    fn set_permissions(path: &Path, perm: Self::Permissions) -> io::Result<()>;
}

pub(crate) trait FilePal<Fs: FsPal + ?Sized>: Sized + Debug {
    fn open(path: &Path, options: &Fs::OpenOptions) -> io::Result<Self>;
    fn read(&self, buf: &mut [u8]) -> io::Result<usize>;
    fn write(&self, buf: &[u8]) -> io::Result<usize>;
    fn flush(&self) -> io::Result<()>;
    fn seek(&self, pos: io::SeekFrom) -> io::Result<u64>;
    fn sync_all(&self) -> io::Result<()>;
    fn sync_data(&self) -> io::Result<()>;
    fn set_len(&self, size: u64) -> io::Result<()>;
    fn metadata(&self) -> io::Result<Fs::Metadata>;
    fn try_clone(&self) -> io::Result<Self>;
    fn set_permissions(&self, perm: Fs::Permissions) -> io::Result<()>;
}

pub(crate) trait MetadataPal<Fs: FsPal + ?Sized>: Clone {
    fn file_type(&self) -> Fs::FileType;
    fn len(&self) -> u64;
    fn permissions(&self) -> Fs::Permissions;
    fn modified(&self) -> io::Result<Fs::SystemTime>;
    fn accessed(&self) -> io::Result<Fs::SystemTime>;
    fn created(&self) -> io::Result<Fs::SystemTime>;
}

pub(crate) trait FileTypePal {
    fn is_dir(&self) -> bool;
    fn is_file(&self) -> bool;
    fn is_symlink(&self) -> bool;
}

pub(crate) trait PermissionsPal {
    fn readonly(&self) -> bool;
    fn set_readonly(&mut self, readonly: bool);
}

pub(crate) trait OpenOptionsPal: Clone + Debug {
    fn new() -> Self;
    fn read(&mut self, read: bool);
    fn write(&mut self, write: bool);
    fn append(&mut self, append: bool);
    fn truncate(&mut self, truncate: bool);
    fn create(&mut self, create: bool);
    fn create_new(&mut self, create_new: bool);
}

pub(crate) trait DirBuilderPal {
    fn new() -> Self;
    fn create(&self, path: &Path) -> io::Result<()>;
}

pub(crate) trait DirEntryPal<Fs: FsPal + ?Sized> {
    fn path(&self) -> PathBuf;
    fn metadata(&self) -> io::Result<Fs::Metadata>;
    fn file_type(&self) -> io::Result<Fs::FileType>;
    fn file_name(&self) -> OsString;
}
