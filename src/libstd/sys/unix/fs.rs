// Copyright 2013-2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use os::unix::prelude::*;

use ffi::{CString, CStr, OsString, OsStr};
use fmt;
use io::{self, Error, ErrorKind, SeekFrom};
use libc::{self, c_int, mode_t};
use mem;
use path::{Path, PathBuf};
use ptr;
use sync::Arc;
use sys::fd::FileDesc;
use sys::time::SystemTime;
use sys::{cvt, cvt_r};
use sys_common::{AsInner, FromInner};
use pal::fs::*;

pub struct Fs;

impl FsPal for Fs {
    type File = File;
    type Metadata = FileAttr;
    type ReadDir = ReadDir;
    type DirEntry = DirEntry;
    type OpenOptions = OpenOptions;
    type Permissions = FilePermissions;
    type FileType = FileType;
    type DirBuilder = DirBuilder;
    type SystemTime = SystemTime;

    fn remove_file(path: &Path) -> io::Result<()> {
        let path = cstr(path)?;
        cvt(unsafe { libc::unlink(path.as_ptr()) })?;
        Ok(())
    }

    fn metadata(path: &Path) -> io::Result<Self::Metadata> {
        let path = cstr(path)?;
        let mut stat: stat64 = unsafe { mem::zeroed() };
        cvt(unsafe {
            stat64(path.as_ptr(), &mut stat as *mut _ as *mut _)
        })?;
        Ok(FileAttr { stat: stat })
    }

    fn symlink_metadata(path: &Path) -> io::Result<Self::Metadata> {
        let path = cstr(path)?;
        let mut stat: stat64 = unsafe { mem::zeroed() };
        cvt(unsafe {
            lstat64(path.as_ptr(), &mut stat as *mut _ as *mut _)
        })?;
        Ok(FileAttr { stat: stat })
    }

    fn rename(from: &Path, to: &Path) -> io::Result<()> {
        let from = cstr(from)?;
        let to = cstr(to)?;
        cvt(unsafe { libc::rename(from.as_ptr(), to.as_ptr()) })?;
        Ok(())
    }

    fn copy(from: &Path, to: &Path) -> io::Result<u64> {
        use fs::{File, set_permissions};
        if !from.is_file() {
            return Err(Error::new(ErrorKind::InvalidInput,
                                  "the source path is not an existing regular file"))
        }

        let mut reader = File::open(from)?;
        let mut writer = File::create(to)?;
        let perm = reader.metadata()?.permissions();

        let ret = io::copy(&mut reader, &mut writer)?;
        set_permissions(to, perm)?;
        Ok(ret)
    }

    fn hard_link(src: &Path, dst: &Path) -> io::Result<()> {
        let src = cstr(src)?;
        let dst = cstr(dst)?;
        cvt(unsafe { libc::link(src.as_ptr(), dst.as_ptr()) })?;
        Ok(())
    }

    fn soft_link(src: &Path, dst: &Path) -> io::Result<()> {
        let src = cstr(src)?;
        let dst = cstr(dst)?;
        cvt(unsafe { libc::symlink(src.as_ptr(), dst.as_ptr()) })?;
        Ok(())
    }

    fn read_link(path: &Path) -> io::Result<PathBuf> {
        let c_path = cstr(path)?;
        let p = c_path.as_ptr();

        let mut buf = Vec::with_capacity(256);

        loop {
            let buf_read = cvt(unsafe {
                libc::readlink(p, buf.as_mut_ptr() as *mut _, buf.capacity())
            })? as usize;

            unsafe { buf.set_len(buf_read); }

            if buf_read != buf.capacity() {
                buf.shrink_to_fit();

                return Ok(PathBuf::from(OsString::from_vec(buf)));
            }

            // Trigger the internal buffer resizing logic of `Vec` by requiring
            // more space than the current capacity. The length is guaranteed to be
            // the same as the capacity due to the if statement above.
            buf.reserve(1);
        }
    }

    fn canonicalize(path: &Path) -> io::Result<PathBuf> {
        let path = CString::new(path.as_os_str().as_bytes())?;
        let buf;
        unsafe {
            let r = libc::realpath(path.as_ptr(), ptr::null_mut());
            if r.is_null() {
                return Err(io::Error::last_os_error())
            }
            buf = CStr::from_ptr(r).to_bytes().to_vec();
            libc::free(r as *mut _);
        }
        Ok(PathBuf::from(OsString::from_vec(buf)))
    }

    fn remove_dir(path: &Path) -> io::Result<()> {
        let path = cstr(path)?;
        cvt(unsafe { libc::rmdir(path.as_ptr()) })?;
        Ok(())
    }

    fn remove_dir_all(path: &Path) -> io::Result<()> {
        let filetype = Fs::symlink_metadata(path)?.file_type();
        if filetype.is_symlink() {
            Fs::remove_file(path)
        } else {
            remove_dir_all_recursive(path)
        }
    }

    fn read_dir(path: &Path) -> io::Result<Self::ReadDir> {
        let root = Arc::new(path.to_path_buf());
        let p = cstr(path)?;
        unsafe {
            let ptr = libc::opendir(p.as_ptr());
            if ptr.is_null() {
                Err(Error::last_os_error())
            } else {
                Ok(ReadDir { dirp: Dir(ptr), root: root })
            }
        }
    }

    fn set_permissions(path: &Path, perm: Self::Permissions) -> io::Result<()> {
        let path = cstr(path)?;
        cvt_r(|| unsafe { libc::chmod(path.as_ptr(), perm.mode) })?;
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "emscripten", target_os = "l4re"))]
use libc::{stat64, fstat64, lstat64, off64_t, ftruncate64, lseek64, dirent64, readdir64_r, open64};
#[cfg(target_os = "android")]
use libc::{stat as stat64, fstat as fstat64, lstat as lstat64, lseek64,
           dirent as dirent64, open as open64};
#[cfg(not(any(target_os = "linux",
              target_os = "emscripten",
              target_os = "l4re",
              target_os = "android")))]
use libc::{stat as stat64, fstat as fstat64, lstat as lstat64, off_t as off64_t,
           ftruncate as ftruncate64, lseek as lseek64, dirent as dirent64, open as open64};
#[cfg(not(any(target_os = "linux",
              target_os = "emscripten",
              target_os = "solaris",
              target_os = "l4re",
              target_os = "fuchsia")))]
use libc::{readdir_r as readdir64_r};

pub struct File(FileDesc);

#[derive(Clone)]
pub struct FileAttr {
    stat: stat64,
}

pub struct ReadDir {
    dirp: Dir,
    root: Arc<PathBuf>,
}

struct Dir(*mut libc::DIR);

unsafe impl Send for Dir {}
unsafe impl Sync for Dir {}

pub struct DirEntry {
    entry: dirent64,
    root: Arc<PathBuf>,
    // We need to store an owned copy of the directory name
    // on Solaris and Fuchsia because a) it uses a zero-length
    // array to store the name, b) its lifetime between readdir
    // calls is not guaranteed.
    #[cfg(any(target_os = "solaris", target_os = "fuchsia"))]
    name: Box<[u8]>
}

#[derive(Clone, Debug)]
pub struct OpenOptions {
    // generic
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
    // system-specific
    custom_flags: i32,
    mode: mode_t,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FilePermissions { mode: mode_t }

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FileType { mode: mode_t }

#[derive(Debug)]
pub struct DirBuilder { mode: mode_t }

impl MetadataPal<Fs> for FileAttr {
    fn file_type(&self) -> <Fs as FsPal>::FileType {
        FileType { mode: self.stat.st_mode as mode_t }
    }

    fn len(&self) -> u64 {
        self.stat.st_size as u64
    }

    fn permissions(&self) -> <Fs as FsPal>::Permissions {
        FilePermissions { mode: (self.stat.st_mode as mode_t) }
    }

    fn modified(&self) -> io::Result<<Fs as FsPal>::SystemTime> {
        self.modified()
    }

    fn accessed(&self) -> io::Result<<Fs as FsPal>::SystemTime> {
        self.accessed()
    }

    fn created(&self) -> io::Result<<Fs as FsPal>::SystemTime> {
        self.created()
    }
}

#[cfg(target_os = "netbsd")]
impl FileAttr {
    fn modified(&self) -> io::Result<SystemTime> {
        Ok(SystemTime::from(libc::timespec {
            tv_sec: self.stat.st_mtime as libc::time_t,
            tv_nsec: self.stat.st_mtimensec as libc::c_long,
        }))
    }

    fn accessed(&self) -> io::Result<SystemTime> {
        Ok(SystemTime::from(libc::timespec {
            tv_sec: self.stat.st_atime as libc::time_t,
            tv_nsec: self.stat.st_atimensec as libc::c_long,
        }))
    }

    fn created(&self) -> io::Result<SystemTime> {
        Ok(SystemTime::from(libc::timespec {
            tv_sec: self.stat.st_birthtime as libc::time_t,
            tv_nsec: self.stat.st_birthtimensec as libc::c_long,
        }))
    }
}

#[cfg(not(target_os = "netbsd"))]
impl FileAttr {
    fn modified(&self) -> io::Result<SystemTime> {
        Ok(SystemTime::from(libc::timespec {
            tv_sec: self.stat.st_mtime as libc::time_t,
            tv_nsec: self.stat.st_mtime_nsec as _,
        }))
    }

    fn accessed(&self) -> io::Result<SystemTime> {
        Ok(SystemTime::from(libc::timespec {
            tv_sec: self.stat.st_atime as libc::time_t,
            tv_nsec: self.stat.st_atime_nsec as _,
        }))
    }

    #[cfg(any(target_os = "bitrig",
              target_os = "freebsd",
              target_os = "openbsd",
              target_os = "macos",
              target_os = "ios"))]
    fn created(&self) -> io::Result<SystemTime> {
        Ok(SystemTime::from(libc::timespec {
            tv_sec: self.stat.st_birthtime as libc::time_t,
            tv_nsec: self.stat.st_birthtime_nsec as libc::c_long,
        }))
    }

    #[cfg(not(any(target_os = "bitrig",
                  target_os = "freebsd",
                  target_os = "openbsd",
                  target_os = "macos",
                  target_os = "ios")))]
    fn created(&self) -> io::Result<SystemTime> {
        Err(io::Error::new(io::ErrorKind::Other,
                           "creation time is not available on this platform \
                            currently"))
    }
}

impl AsInner<stat64> for FileAttr {
    fn as_inner(&self) -> &stat64 { &self.stat }
}

impl PermissionsPal for FilePermissions {
    fn readonly(&self) -> bool {
        // check if any class (owner, group, others) has write permission
        self.mode & 0o222 == 0
    }

    fn set_readonly(&mut self, readonly: bool) {
        if readonly {
            // remove write permission for all classes; equivalent to `chmod a-w <file>`
            self.mode &= !0o222;
        } else {
            // add write permission for all classes; equivalent to `chmod a+w <file>`
            self.mode |= 0o222;
        }
    }
}

impl FilePermissions {
    pub(in super) fn mode(&self) -> u32 { self.mode as u32 }
}

impl FileTypePal for FileType {
    fn is_dir(&self) -> bool { self.is(libc::S_IFDIR) }
    fn is_file(&self) -> bool { self.is(libc::S_IFREG) }
    fn is_symlink(&self) -> bool { self.is(libc::S_IFLNK) }
}

impl FileType {
    pub(in super) fn is(&self, mode: mode_t) -> bool { self.mode & libc::S_IFMT == mode }
}

impl FromInner<u32> for FilePermissions {
    fn from_inner(mode: u32) -> FilePermissions {
        FilePermissions { mode: mode as mode_t }
    }
}

impl fmt::Debug for ReadDir {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // This will only be called from std::fs::ReadDir, which will add a "ReadDir()" frame.
        // Thus the result will be e g 'ReadDir("/home")'
        fmt::Debug::fmt(&*self.root, f)
    }
}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;

    #[cfg(any(target_os = "solaris", target_os = "fuchsia"))]
    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        unsafe {
            loop {
                // Although readdir_r(3) would be a correct function to use here because
                // of the thread safety, on Illumos and Fuchsia the readdir(3C) function
                // is safe to use in threaded applications and it is generally preferred
                // over the readdir_r(3C) function.
                super::os::set_errno(0);
                let entry_ptr = libc::readdir(self.dirp.0);
                if entry_ptr.is_null() {
                    // NULL can mean either the end is reached or an error occurred.
                    // So we had to clear errno beforehand to check for an error now.
                    return match super::os::errno() {
                        0 => None,
                        e => Some(Err(Error::from_raw_os_error(e))),
                    }
                }

                let name = (*entry_ptr).d_name.as_ptr();
                let namelen = libc::strlen(name) as usize;

                let ret = DirEntry {
                    entry: *entry_ptr,
                    name: ::slice::from_raw_parts(name as *const u8,
                                                  namelen as usize).to_owned().into_boxed_slice(),
                    root: self.root.clone()
                };
                if ret.name_bytes() != b"." && ret.name_bytes() != b".." {
                    return Some(Ok(ret))
                }
            }
        }
    }

    #[cfg(not(any(target_os = "solaris", target_os = "fuchsia")))]
    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        unsafe {
            let mut ret = DirEntry {
                entry: mem::zeroed(),
                root: self.root.clone()
            };
            let mut entry_ptr = ptr::null_mut();
            loop {
                if readdir64_r(self.dirp.0, &mut ret.entry, &mut entry_ptr) != 0 {
                    return Some(Err(Error::last_os_error()))
                }
                if entry_ptr.is_null() {
                    return None
                }
                if ret.name_bytes() != b"." && ret.name_bytes() != b".." {
                    return Some(Ok(ret))
                }
            }
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let r = unsafe { libc::closedir(self.0) };
        debug_assert_eq!(r, 0);
    }
}

impl DirEntryPal<Fs> for DirEntry {
    fn path(&self) -> PathBuf {
        self.root.join(OsStr::from_bytes(self.name_bytes()))
    }

    fn metadata(&self) -> io::Result<<Fs as FsPal>::Metadata> {
        Fs::symlink_metadata(&self.path())
    }

    #[cfg(any(target_os = "solaris", target_os = "haiku"))]
    fn file_type(&self) -> io::Result<<Fs as FsPal>::FileType> {
        Fs::symlink_metadata(&self.path()).map(|m| m.file_type())
    }

    #[cfg(not(any(target_os = "solaris", target_os = "haiku")))]
    fn file_type(&self) -> io::Result<<Fs as FsPal>::FileType> {
        match self.entry.d_type {
            libc::DT_CHR => Ok(FileType { mode: libc::S_IFCHR }),
            libc::DT_FIFO => Ok(FileType { mode: libc::S_IFIFO }),
            libc::DT_LNK => Ok(FileType { mode: libc::S_IFLNK }),
            libc::DT_REG => Ok(FileType { mode: libc::S_IFREG }),
            libc::DT_SOCK => Ok(FileType { mode: libc::S_IFSOCK }),
            libc::DT_DIR => Ok(FileType { mode: libc::S_IFDIR }),
            libc::DT_BLK => Ok(FileType { mode: libc::S_IFBLK }),
            _ => Fs::symlink_metadata(&self.path()).map(|m| m.file_type()),
        }
    }

    fn file_name(&self) -> OsString {
        OsStr::from_bytes(self.name_bytes()).to_os_string()
    }
}

impl DirEntry {
    #[cfg(any(target_os = "macos",
              target_os = "ios",
              target_os = "linux",
              target_os = "emscripten",
              target_os = "android",
              target_os = "solaris",
              target_os = "haiku",
              target_os = "l4re",
              target_os = "fuchsia"))]
    pub(in super) fn ino(&self) -> u64 {
        self.entry.d_ino as u64
    }

    #[cfg(any(target_os = "freebsd",
              target_os = "openbsd",
              target_os = "bitrig",
              target_os = "netbsd",
              target_os = "dragonfly"))]
    pub(in super) fn ino(&self) -> u64 {
        self.entry.d_fileno as u64
    }

    #[cfg(any(target_os = "macos",
              target_os = "ios",
              target_os = "netbsd",
              target_os = "openbsd",
              target_os = "freebsd",
              target_os = "dragonfly",
              target_os = "bitrig"))]
    fn name_bytes(&self) -> &[u8] {
        unsafe {
            ::slice::from_raw_parts(self.entry.d_name.as_ptr() as *const u8,
                                    self.entry.d_namlen as usize)
        }
    }
    #[cfg(any(target_os = "android",
              target_os = "linux",
              target_os = "emscripten",
              target_os = "l4re",
              target_os = "haiku"))]
    fn name_bytes(&self) -> &[u8] {
        unsafe {
            CStr::from_ptr(self.entry.d_name.as_ptr()).to_bytes()
        }
    }
    #[cfg(any(target_os = "solaris",
              target_os = "fuchsia"))]
    fn name_bytes(&self) -> &[u8] {
        &*self.name
    }
}

impl OpenOptionsPal for OpenOptions {
    fn new() -> Self {
        OpenOptions {
            // generic
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
            create_new: false,
            // system-specific
            custom_flags: 0,
            mode: 0o666,
        }
    }

    fn read(&mut self, read: bool) {  self.read = read; }
    fn write(&mut self, write: bool) { self.write = write; }
    fn append(&mut self, append: bool) { self.append = append; }
    fn truncate(&mut self, truncate: bool) { self.truncate = truncate; }
    fn create(&mut self, create: bool) { self.create = create; }
    fn create_new(&mut self, create_new: bool) { self.create_new = create_new; }
}

impl OpenOptions {
    pub(in super) fn custom_flags(&mut self, flags: i32) { self.custom_flags = flags; }
    pub(in super) fn mode(&mut self, mode: u32) { self.mode = mode as mode_t; }

    fn get_access_mode(&self) -> io::Result<c_int> {
        match (self.read, self.write, self.append) {
            (true,  false, false) => Ok(libc::O_RDONLY),
            (false, true,  false) => Ok(libc::O_WRONLY),
            (true,  true,  false) => Ok(libc::O_RDWR),
            (false, _,     true)  => Ok(libc::O_WRONLY | libc::O_APPEND),
            (true,  _,     true)  => Ok(libc::O_RDWR | libc::O_APPEND),
            (false, false, false) => Err(Error::from_raw_os_error(libc::EINVAL)),
        }
    }

    fn get_creation_mode(&self) -> io::Result<c_int> {
        match (self.write, self.append) {
            (true, false) => {}
            (false, false) =>
                if self.truncate || self.create || self.create_new {
                    return Err(Error::from_raw_os_error(libc::EINVAL));
                },
            (_, true) =>
                if self.truncate && !self.create_new {
                    return Err(Error::from_raw_os_error(libc::EINVAL));
                },
        }

        Ok(match (self.create, self.truncate, self.create_new) {
                (false, false, false) => 0,
                (true,  false, false) => libc::O_CREAT,
                (false, true,  false) => libc::O_TRUNC,
                (true,  true,  false) => libc::O_CREAT | libc::O_TRUNC,
                (_,      _,    true)  => libc::O_CREAT | libc::O_EXCL,
           })
    }
}

impl FilePal<Fs> for File {
    fn open(path: &Path, options: &<Fs as FsPal>::OpenOptions) -> io::Result<Self> {
        let path = cstr(path)?;
        File::open_c(&path, options)
    }

    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }

    fn write(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }

    fn seek(&self, pos: io::SeekFrom) -> io::Result<u64> {
        let (whence, pos) = match pos {
            // Casting to `i64` is fine, too large values will end up as
            // negative which will cause an error in `lseek64`.
            SeekFrom::Start(off) => (libc::SEEK_SET, off as i64),
            SeekFrom::End(off) => (libc::SEEK_END, off),
            SeekFrom::Current(off) => (libc::SEEK_CUR, off),
        };
        #[cfg(target_os = "emscripten")]
        let pos = pos as i32;
        let n = cvt(unsafe { lseek64(self.0.raw(), pos, whence) })?;
        Ok(n as u64)
    }

    fn sync_all(&self) -> io::Result<()> {
        cvt_r(|| unsafe { libc::fsync(self.0.raw()) })?;
        Ok(())
    }

    fn sync_data(&self) -> io::Result<()> {
        cvt_r(|| unsafe { os_datasync(self.0.raw()) })?;
        return Ok(());

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        unsafe fn os_datasync(fd: c_int) -> c_int {
            libc::fcntl(fd, libc::F_FULLFSYNC)
        }
        #[cfg(target_os = "linux")]
        unsafe fn os_datasync(fd: c_int) -> c_int { libc::fdatasync(fd) }
        #[cfg(not(any(target_os = "macos",
                      target_os = "ios",
                      target_os = "linux")))]
        unsafe fn os_datasync(fd: c_int) -> c_int { libc::fsync(fd) }
    }

    fn set_len(&self, size: u64) -> io::Result<()> {
        #[cfg(target_os = "android")]
        return ::sys::android::ftruncate64(self.0.raw(), size);

        #[cfg(not(target_os = "android"))]
        return cvt_r(|| unsafe {
            ftruncate64(self.0.raw(), size as off64_t)
        }).map(|_| ());
    }

    fn metadata(&self) -> io::Result<<Fs as FsPal>::Metadata> {
        let mut stat: stat64 = unsafe { mem::zeroed() };
        cvt(unsafe {
            fstat64(self.0.raw(), &mut stat)
        })?;
        Ok(FileAttr { stat: stat })
    }

    fn try_clone(&self) -> io::Result<Self> {
        self.0.duplicate().map(File)
    }

    fn set_permissions(&self, perm: <Fs as FsPal>::Permissions) -> io::Result<()> {
        cvt_r(|| unsafe { libc::fchmod(self.0.raw(), perm.mode) })?;
        Ok(())
    }

}

impl File {
    pub(in super) fn open_c(path: &CStr, opts: &OpenOptions) -> io::Result<File> {
        let flags = libc::O_CLOEXEC |
                    opts.get_access_mode()? |
                    opts.get_creation_mode()? |
                    (opts.custom_flags as c_int & !libc::O_ACCMODE);
        let fd = cvt_r(|| unsafe {
            open64(path.as_ptr(), flags, opts.mode as c_int)
        })?;
        let fd = FileDesc::new(fd);

        // Currently the standard library supports Linux 2.6.18 which did not
        // have the O_CLOEXEC flag (passed above). If we're running on an older
        // Linux kernel then the flag is just ignored by the OS, so we continue
        // to explicitly ask for a CLOEXEC fd here.
        //
        // The CLOEXEC flag, however, is supported on versions of macOS/BSD/etc
        // that we support, so we only do this on Linux currently.
        if cfg!(target_os = "linux") {
            fd.set_cloexec()?;
        }

        Ok(File(fd))
    }

    pub(in super) fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        self.0.read_at(buf, offset)
    }

    pub(in super) fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<usize> {
        self.0.write_at(buf, offset)
    }

    pub(in super) fn fd(&self) -> &FileDesc { &self.0 }

    pub(in super) fn into_fd(self) -> FileDesc { self.0 }
}

impl DirBuilderPal for DirBuilder {
    fn new() -> Self {
        DirBuilder { mode: 0o777 }
    }

    fn create(&self, path: &Path) -> io::Result<()> {
        let path = cstr(path)?;
        cvt(unsafe { libc::mkdir(path.as_ptr(), self.mode) })?;
        Ok(())
    }
}

impl DirBuilder {
    pub(in super) fn set_mode(&mut self, mode: u32) {
        self.mode = mode as mode_t;
    }
}

fn cstr(path: &Path) -> io::Result<CString> {
    Ok(CString::new(path.as_os_str().as_bytes())?)
}

impl FromInner<c_int> for File {
    fn from_inner(fd: c_int) -> File {
        File(FileDesc::new(fd))
    }
}

impl fmt::Debug for File {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        #[cfg(target_os = "linux")]
        fn get_path(fd: c_int) -> Option<PathBuf> {
            let mut p = PathBuf::from("/proc/self/fd");
            p.push(&fd.to_string());
            Fs::read_link(&p).ok()
        }

        #[cfg(target_os = "macos")]
        fn get_path(fd: c_int) -> Option<PathBuf> {
            // FIXME: The use of PATH_MAX is generally not encouraged, but it
            // is inevitable in this case because macOS defines `fcntl` with
            // `F_GETPATH` in terms of `MAXPATHLEN`, and there are no
            // alternatives. If a better method is invented, it should be used
            // instead.
            let mut buf = vec![0;libc::PATH_MAX as usize];
            let n = unsafe { libc::fcntl(fd, libc::F_GETPATH, buf.as_ptr()) };
            if n == -1 {
                return None;
            }
            let l = buf.iter().position(|&c| c == 0).unwrap();
            buf.truncate(l as usize);
            buf.shrink_to_fit();
            Some(PathBuf::from(OsString::from_vec(buf)))
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        fn get_path(_fd: c_int) -> Option<PathBuf> {
            // FIXME(#24570): implement this for other Unix platforms
            None
        }

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        fn get_mode(fd: c_int) -> Option<(bool, bool)> {
            let mode = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if mode == -1 {
                return None;
            }
            match mode & libc::O_ACCMODE {
                libc::O_RDONLY => Some((true, false)),
                libc::O_RDWR => Some((true, true)),
                libc::O_WRONLY => Some((false, true)),
                _ => None
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        fn get_mode(_fd: c_int) -> Option<(bool, bool)> {
            // FIXME(#24570): implement this for other Unix platforms
            None
        }

        let fd = self.0.raw();
        let mut b = f.debug_struct("File");
        b.field("fd", &fd);
        if let Some(path) = get_path(fd) {
            b.field("path", &path);
        }
        if let Some((read, write)) = get_mode(fd) {
            b.field("read", &read).field("write", &write);
        }
        b.finish()
    }
}

fn remove_dir_all_recursive(path: &Path) -> io::Result<()> {
    for child in Fs::read_dir(path)? {
        let child = child?;
        if child.file_type()?.is_dir() {
            remove_dir_all_recursive(&child.path())?;
        } else {
            Fs::remove_file(&child.path())?;
        }
    }
    Fs::remove_dir(path)
}
