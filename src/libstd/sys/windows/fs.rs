// Copyright 2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use os::windows::prelude::*;

use ffi::OsString;
use fmt;
use io::{self, Error, SeekFrom};
use mem;
use path::{Path, PathBuf};
use ptr;
use slice;
use sync::Arc;
use sys::handle::Handle;
use sys::time::SystemTime;
use sys::{c, cvt};
use sys_common::FromInner;
use pal::fs::*;

use super::to_u16s;

pub struct Fs;

impl FsPal for Fs {
    type File = File;
    type Metadata = FileAttr;
    type FileType = FileType;
    type Permissions = FilePermissions;
    type OpenOptions = OpenOptions;
    type DirBuilder = DirBuilder;
    type DirEntry = DirEntry;
    type ReadDir = ReadDir;
    type SystemTime = SystemTime;

    fn remove_file(path: &Path) -> io::Result<()> {
        let p_u16s = to_u16s(path)?;
        cvt(unsafe { c::DeleteFileW(p_u16s.as_ptr()) })?;
        Ok(())
    }

    fn metadata(path: &Path) -> io::Result<Self::Metadata> {
        let mut opts = OpenOptions::new();
        // No read or write permissions are necessary
        opts.access_mode(0);
        // This flag is so we can open directories too
        opts.custom_flags(c::FILE_FLAG_BACKUP_SEMANTICS);
        let file = File::open(path, &opts)?;
        file.metadata()
    }

    fn symlink_metadata(path: &Path) -> io::Result<Self::Metadata> {
        let mut opts = OpenOptions::new();
        // No read or write permissions are necessary
        opts.access_mode(0);
        opts.custom_flags(c::FILE_FLAG_BACKUP_SEMANTICS | c::FILE_FLAG_OPEN_REPARSE_POINT);
        let file = File::open(path, &opts)?;
        file.metadata()
    }

    fn rename(from: &Path, to: &Path) -> io::Result<()> {
        let from = to_u16s(from)?;
        let to = to_u16s(to)?;
        cvt(unsafe {
            c::MoveFileExW(from.as_ptr(), to.as_ptr(), c::MOVEFILE_REPLACE_EXISTING)
        })?;
        Ok(())
    }

    fn copy(from: &Path, to: &Path) -> io::Result<u64> {
        unsafe extern "system" fn callback(
            _TotalFileSize: c::LARGE_INTEGER,
            TotalBytesTransferred: c::LARGE_INTEGER,
            _StreamSize: c::LARGE_INTEGER,
            _StreamBytesTransferred: c::LARGE_INTEGER,
            _dwStreamNumber: c::DWORD,
            _dwCallbackReason: c::DWORD,
            _hSourceFile: c::HANDLE,
            _hDestinationFile: c::HANDLE,
            lpData: c::LPVOID,
        ) -> c::DWORD {
            if dwStreamNumber == 1 {*(lpData as *mut i64) = StreamBytesTransferred;}
            c::PROGRESS_CONTINUE
        }
        let pfrom = to_u16s(from)?;
        let pto = to_u16s(to)?;
        let mut size = 0i64;
        cvt(unsafe {
            c::CopyFileExW(pfrom.as_ptr(), pto.as_ptr(), Some(callback),
                        &mut size as *mut _ as *mut _, ptr::null_mut(), 0)
        })?;
        Ok(size as u64)
    }

    fn hard_link(src: &Path, dst: &Path) -> io::Result<()> {
        let src = to_u16s(src)?;
        let dst = to_u16s(dst)?;
        cvt(unsafe {
            c::CreateHardLinkW(dst.as_ptr(), src.as_ptr(), ptr::null_mut())
        })?;
        Ok(())
    }

    fn soft_link(src: &Path, dst: &Path) -> io::Result<()> {
        symlink_inner(src, dst, false)
    }

    fn read_link(path: &Path) -> io::Result<PathBuf> {
        // Open the link with no access mode, instead of generic read.
        // By default FILE_LIST_DIRECTORY is denied for the junction "C:\Documents and Settings", so
        // this is needed for a common case.
        let mut opts = OpenOptions::new();
        opts.access_mode(0);
        opts.custom_flags(c::FILE_FLAG_OPEN_REPARSE_POINT |
                          c::FILE_FLAG_BACKUP_SEMANTICS);
        let file = File::open(&path, &opts)?;
        file.readlink()
    }

    fn canonicalize(path: &Path) -> io::Result<PathBuf> {
        let mut opts = OpenOptions::new();
        // No read or write permissions are necessary
        opts.access_mode(0);
        // This flag is so we can open directories too
        opts.custom_flags(c::FILE_FLAG_BACKUP_SEMANTICS);
        let f = File::open(path, &opts)?;
        get_path(&f)
    }

    fn remove_dir(path: &Path) -> io::Result<()> {
        let path = to_u16s(path)?;
        cvt(unsafe { c::RemoveDirectoryW(path.as_ptr()) })?;
        Ok(())
    }

    fn remove_dir_all(path: &Path) -> io::Result<()> {
        let filetype = Fs::symlink_metadata(path)?.file_type();
        if filetype.is_symlink() {
            // On Windows symlinks to files and directories are removed differently.
            // rmdir only deletes dir symlinks and junctions, not file symlinks.
            Fs::remove_dir(path)
        } else {
            remove_dir_all_recursive(path)
        }
    }

    fn read_dir(path: &Path) -> io::Result<Self::ReadDir> {
        let root = path.to_path_buf();
        let star = path.join("*");
        let path = to_u16s(&star)?;

        unsafe {
            let mut wfd = mem::zeroed();
            let find_handle = c::FindFirstFileW(path.as_ptr(), &mut wfd);
            if find_handle != c::INVALID_HANDLE_VALUE {
                Ok(ReadDir {
                    handle: FindNextFileHandle(find_handle),
                    root: Arc::new(root),
                    first: Some(wfd),
                })
            } else {
                Err(Error::last_os_error())
            }
        }
    }

    fn set_permissions(path: &Path, perm: Self::Permissions) -> io::Result<()> {
        let path = to_u16s(path)?;
        unsafe {
            cvt(c::SetFileAttributesW(path.as_ptr(), perm.attrs))?;
            Ok(())
        }
    }
}

pub struct File { handle: Handle }

#[derive(Clone)]
pub struct FileAttr {
    attributes: c::DWORD,
    creation_time: c::FILETIME,
    last_access_time: c::FILETIME,
    last_write_time: c::FILETIME,
    file_size: u64,
    reparse_tag: c::DWORD,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum FileType {
    Dir, File, SymlinkFile, SymlinkDir, ReparsePoint, MountPoint,
}

pub struct ReadDir {
    handle: FindNextFileHandle,
    root: Arc<PathBuf>,
    first: Option<c::WIN32_FIND_DATAW>,
}

struct FindNextFileHandle(c::HANDLE);

unsafe impl Send for FindNextFileHandle {}
unsafe impl Sync for FindNextFileHandle {}

pub struct DirEntry {
    root: Arc<PathBuf>,
    data: c::WIN32_FIND_DATAW,
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
    custom_flags: u32,
    access_mode: Option<c::DWORD>,
    attributes: c::DWORD,
    share_mode: c::DWORD,
    security_qos_flags: c::DWORD,
    security_attributes: usize, // FIXME: should be a reference
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FilePermissions { attrs: c::DWORD }

impl PermissionsPal for FilePermissions {
    fn readonly(&self) -> bool {
        self.attrs & c::FILE_ATTRIBUTE_READONLY != 0
    }

    fn set_readonly(&mut self, readonly: bool) {
        if readonly {
            self.attrs |= c::FILE_ATTRIBUTE_READONLY;
        } else {
            self.attrs &= !c::FILE_ATTRIBUTE_READONLY;
        }
    }
}

#[derive(Debug)]
pub struct DirBuilder;

impl DirBuilderPal for DirBuilder {
    fn new() -> Self { DirBuilder }

    fn create(&self, path: &Path) -> io::Result<()> {
        let path = to_u16s(path)?;
        cvt(unsafe {
            c::CreateDirectoryW(path.as_ptr(), ptr::null_mut())
        })?;
        Ok(())
    }
}

impl fmt::Debug for ReadDir {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // This will only be called from std::fs::ReadDir, which will add a "ReadDir()" frame.
        // Thus the result will be e g 'ReadDir("C:\")'
        fmt::Debug::fmt(&*self.root, f)
    }
}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;
    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        if let Some(first) = self.first.take() {
            if let Some(e) = DirEntry::new(&self.root, &first) {
                return Some(Ok(e));
            }
        }
        unsafe {
            let mut wfd = mem::zeroed();
            loop {
                if c::FindNextFileW(self.handle.0, &mut wfd) == 0 {
                    if c::GetLastError() == c::ERROR_NO_MORE_FILES {
                        return None
                    } else {
                        return Some(Err(Error::last_os_error()))
                    }
                }
                if let Some(e) = DirEntry::new(&self.root, &wfd) {
                    return Some(Ok(e))
                }
            }
        }
    }
}

impl Drop for FindNextFileHandle {
    fn drop(&mut self) {
        let r = unsafe { c::FindClose(self.0) };
        debug_assert!(r != 0);
    }
}

impl DirEntryPal<Fs> for DirEntry {
    fn path(&self) -> PathBuf {
        self.root.join(&self.file_name())
    }

    fn metadata(&self) -> io::Result<<Fs as FsPal>::Metadata> {
        Ok(FileAttr {
            attributes: self.data.dwFileAttributes,
            creation_time: self.data.ftCreationTime,
            last_access_time: self.data.ftLastAccessTime,
            last_write_time: self.data.ftLastWriteTime,
            file_size: ((self.data.nFileSizeHigh as u64) << 32) | (self.data.nFileSizeLow as u64),
            reparse_tag: if self.data.dwFileAttributes & c::FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    // reserved unless this is a reparse point
                    self.data.dwReserved0
                } else {
                    0
                },
        })
    }

    fn file_type(&self) -> io::Result<<Fs as FsPal>::FileType> {
        Ok(FileType::new(self.data.dwFileAttributes,
                         /* reparse_tag = */ self.data.dwReserved0))
    }

    fn file_name(&self) -> OsString {
        let filename = super::truncate_utf16_at_nul(&self.data.cFileName);
        OsString::from_wide(filename)
    }
}

impl DirEntry {
    fn new(root: &Arc<PathBuf>, wfd: &c::WIN32_FIND_DATAW) -> Option<DirEntry> {
        match &wfd.cFileName[0..3] {
            // check for '.' and '..'
            &[46, 0, ..] |
            &[46, 46, 0, ..] => return None,
            _ => {}
        }

        Some(DirEntry {
            root: root.clone(),
            data: *wfd,
        })
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
            access_mode: None,
            share_mode: c::FILE_SHARE_READ | c::FILE_SHARE_WRITE | c::FILE_SHARE_DELETE,
            attributes: 0,
            security_qos_flags: 0,
            security_attributes: 0,
        }
    }

    fn read(&mut self, read: bool) { self.read = read; }
    fn write(&mut self, write: bool) { self.write = write;}
    fn append(&mut self, append: bool) { self.append = append; }
    fn truncate(&mut self, truncate: bool) { self.truncate = truncate; }
    fn create(&mut self, create: bool) { self.create = create; }
    fn create_new(&mut self, create_new: bool) { self.create_new = create_new; }
}

impl OpenOptions {
    pub(in super) fn custom_flags(&mut self, flags: u32) { self.custom_flags = flags; }
    pub(in super) fn access_mode(&mut self, access_mode: u32) { self.access_mode = Some(access_mode); }
    pub(in super) fn share_mode(&mut self, share_mode: u32) { self.share_mode = share_mode; }
    pub(in super) fn attributes(&mut self, attrs: u32) { self.attributes = attrs; }
    pub(in super) fn security_qos_flags(&mut self, flags: u32) { self.security_qos_flags = flags; }
    pub(in super) fn security_attributes(&mut self, attrs: c::LPSECURITY_ATTRIBUTES) {
        self.security_attributes = attrs as usize;
    }

    fn get_access_mode(&self) -> io::Result<c::DWORD> {
        const ERROR_INVALID_PARAMETER: i32 = 87;

        match (self.read, self.write, self.append, self.access_mode) {
            (.., Some(mode)) => Ok(mode),
            (true,  false, false, None) => Ok(c::GENERIC_READ),
            (false, true,  false, None) => Ok(c::GENERIC_WRITE),
            (true,  true,  false, None) => Ok(c::GENERIC_READ | c::GENERIC_WRITE),
            (false, _,     true,  None) => Ok(c::FILE_GENERIC_WRITE & !c::FILE_WRITE_DATA),
            (true,  _,     true,  None) => Ok(c::GENERIC_READ |
                                              (c::FILE_GENERIC_WRITE & !c::FILE_WRITE_DATA)),
            (false, false, false, None) => Err(Error::from_raw_os_error(ERROR_INVALID_PARAMETER)),
        }
    }

    fn get_creation_mode(&self) -> io::Result<c::DWORD> {
        const ERROR_INVALID_PARAMETER: i32 = 87;

        match (self.write, self.append) {
            (true, false) => {}
            (false, false) =>
                if self.truncate || self.create || self.create_new {
                    return Err(Error::from_raw_os_error(ERROR_INVALID_PARAMETER));
                },
            (_, true) =>
                if self.truncate && !self.create_new {
                    return Err(Error::from_raw_os_error(ERROR_INVALID_PARAMETER));
                },
        }

        Ok(match (self.create, self.truncate, self.create_new) {
                (false, false, false) => c::OPEN_EXISTING,
                (true,  false, false) => c::OPEN_ALWAYS,
                (false, true,  false) => c::TRUNCATE_EXISTING,
                (true,  true,  false) => c::CREATE_ALWAYS,
                (_,      _,    true)  => c::CREATE_NEW,
           })
    }

    fn get_flags_and_attributes(&self) -> c::DWORD {
        self.custom_flags |
        self.attributes |
        self.security_qos_flags |
        if self.security_qos_flags != 0 { c::SECURITY_SQOS_PRESENT } else { 0 } |
        if self.create_new { c::FILE_FLAG_OPEN_REPARSE_POINT } else { 0 }
    }
}

impl FilePal<Fs> for File {
    fn open(path: &Path, options: &<Fs as FsPal>::OpenOptions) -> io::Result<Self> {
        let path = to_u16s(path)?;
        let handle = unsafe {
            c::CreateFileW(path.as_ptr(),
                           options.get_access_mode()?,
                           options.share_mode,
                           options.security_attributes as *mut _,
                           options.get_creation_mode()?,
                           options.get_flags_and_attributes(),
                           ptr::null_mut())
        };
        if handle == c::INVALID_HANDLE_VALUE {
            Err(Error::last_os_error())
        } else {
            Ok(File { handle: Handle::new(handle) })
        }
    }

    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.handle.read(buf)
    }

    fn write(&self, buf: &[u8]) -> io::Result<usize> {
        self.handle.write(buf)
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }

    fn seek(&self, pos: io::SeekFrom) -> io::Result<u64> {
        let (whence, pos) = match pos {
            // Casting to `i64` is fine, `SetFilePointerEx` reinterprets this
            // integer as `u64`.
            SeekFrom::Start(n) => (c::FILE_BEGIN, n as i64),
            SeekFrom::End(n) => (c::FILE_END, n),
            SeekFrom::Current(n) => (c::FILE_CURRENT, n),
        };
        let pos = pos as c::LARGE_INTEGER;
        let mut newpos = 0;
        cvt(unsafe {
            c::SetFilePointerEx(self.handle.raw(), pos,
                                &mut newpos, whence)
        })?;
        Ok(newpos as u64)
    }

    fn sync_all(&self) -> io::Result<()> {
        cvt(unsafe { c::FlushFileBuffers(self.handle.raw()) })?;
        Ok(())
    }

    fn sync_data(&self) -> io::Result<()> {
        self.sync_all()
    }

    fn set_len(&self, size: u64) -> io::Result<()> {
        let mut info = c::FILE_END_OF_FILE_INFO {
            EndOfFile: size as c::LARGE_INTEGER,
        };
        let size = mem::size_of_val(&info);
        cvt(unsafe {
            c::SetFileInformationByHandle(self.handle.raw(),
                                          c::FileEndOfFileInfo,
                                          &mut info as *mut _ as *mut _,
                                          size as c::DWORD)
        })?;
        Ok(())
    }

    fn metadata(&self) -> io::Result<<Fs as FsPal>::Metadata> {
        unsafe {
            let mut info: c::BY_HANDLE_FILE_INFORMATION = mem::zeroed();
            cvt(c::GetFileInformationByHandle(self.handle.raw(),
                                              &mut info))?;
            let mut attr = FileAttr {
                attributes: info.dwFileAttributes,
                creation_time: info.ftCreationTime,
                last_access_time: info.ftLastAccessTime,
                last_write_time: info.ftLastWriteTime,
                file_size: ((info.nFileSizeHigh as u64) << 32) | (info.nFileSizeLow as u64),
                reparse_tag: 0,
            };
            if attr.is_reparse_point() {
                let mut b = [0; c::MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
                if let Ok((_, buf)) = self.reparse_point(&mut b) {
                    attr.reparse_tag = buf.ReparseTag;
                }
            }
            Ok(attr)
        }
    }

    fn try_clone(&self) -> io::Result<Self> {
        Ok(File {
            handle: self.handle.duplicate(0, true, c::DUPLICATE_SAME_ACCESS)?,
        })
    }

    fn set_permissions(&self, perm: <Fs as FsPal>::Permissions) -> io::Result<()> {
        let mut info = c::FILE_BASIC_INFO {
            CreationTime: 0,
            LastAccessTime: 0,
            LastWriteTime: 0,
            ChangeTime: 0,
            FileAttributes: perm.attrs,
        };
        let size = mem::size_of_val(&info);
        cvt(unsafe {
            c::SetFileInformationByHandle(self.handle.raw(),
                                          c::FileBasicInfo,
                                          &mut info as *mut _ as *mut _,
                                          size as c::DWORD)
        })?;
        Ok(())
    }

}

impl File {
    pub(in super) fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        self.handle.read_at(buf, offset)
    }

    pub(in super) fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<usize> {
        self.handle.write_at(buf, offset)
    }

    pub(in super) fn handle(&self) -> &Handle { &self.handle }

    pub(in super) fn into_handle(self) -> Handle { self.handle }

    fn reparse_point<'a>(&self,
                         space: &'a mut [u8; c::MAXIMUM_REPARSE_DATA_BUFFER_SIZE])
                         -> io::Result<(c::DWORD, &'a c::REPARSE_DATA_BUFFER)> {
        unsafe {
            let mut bytes = 0;
            cvt({
                c::DeviceIoControl(self.handle.raw(),
                                   c::FSCTL_GET_REPARSE_POINT,
                                   ptr::null_mut(),
                                   0,
                                   space.as_mut_ptr() as *mut _,
                                   space.len() as c::DWORD,
                                   &mut bytes,
                                   ptr::null_mut())
            })?;
            Ok((bytes, &*(space.as_ptr() as *const c::REPARSE_DATA_BUFFER)))
        }
    }

    fn readlink(&self) -> io::Result<PathBuf> {
        let mut space = [0u8; c::MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
        let (_bytes, buf) = self.reparse_point(&mut space)?;
        unsafe {
            let (path_buffer, subst_off, subst_len, relative) = match buf.ReparseTag {
                c::IO_REPARSE_TAG_SYMLINK => {
                    let info: *const c::SYMBOLIC_LINK_REPARSE_BUFFER =
                        &buf.rest as *const _ as *const _;
                    (&(*info).PathBuffer as *const _ as *const u16,
                     (*info).SubstituteNameOffset / 2,
                     (*info).SubstituteNameLength / 2,
                     (*info).Flags & c::SYMLINK_FLAG_RELATIVE != 0)
                },
                c::IO_REPARSE_TAG_MOUNT_POINT => {
                    let info: *const c::MOUNT_POINT_REPARSE_BUFFER =
                        &buf.rest as *const _ as *const _;
                    (&(*info).PathBuffer as *const _ as *const u16,
                     (*info).SubstituteNameOffset / 2,
                     (*info).SubstituteNameLength / 2,
                     false)
                },
                _ => return Err(io::Error::new(io::ErrorKind::Other,
                                               "Unsupported reparse point type"))
            };
            let subst_ptr = path_buffer.offset(subst_off as isize);
            let mut subst = slice::from_raw_parts(subst_ptr, subst_len as usize);
            // Absolute paths start with an NT internal namespace prefix `\??\`
            // We should not let it leak through.
            if !relative && subst.starts_with(&[92u16, 63u16, 63u16, 92u16]) {
                subst = &subst[4..];
            }
            Ok(PathBuf::from(OsString::from_wide(subst)))
        }
    }
}

impl FromInner<c::HANDLE> for File {
    fn from_inner(handle: c::HANDLE) -> File {
        File { handle: Handle::new(handle) }
    }
}

impl fmt::Debug for File {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // FIXME(#24570): add more info here (e.g. mode)
        let mut b = f.debug_struct("File");
        b.field("handle", &self.handle.raw());
        if let Ok(path) = get_path(&self) {
            b.field("path", &path);
        }
        b.finish()
    }
}

impl MetadataPal<Fs> for FileAttr {
    fn file_type(&self) -> <Fs as FsPal>::FileType {
        FileType::new(self.attributes, self.reparse_tag)
    }

    fn len(&self) -> u64 {
        self.file_size
    }

    fn permissions(&self) -> <Fs as FsPal>::Permissions {
        FilePermissions { attrs: self.attributes }
    }

    fn modified(&self) -> io::Result<<Fs as FsPal>::SystemTime> {
        Ok(SystemTime::from(self.last_write_time))
    }

    fn accessed(&self) -> io::Result<<Fs as FsPal>::SystemTime> {
        Ok(SystemTime::from(self.last_access_time))
    }

    fn created(&self) -> io::Result<<Fs as FsPal>::SystemTime> {
        Ok(SystemTime::from(self.creation_time))
    }
}

impl FileAttr {
    pub(in super) fn attrs(&self) -> u32 { self.attributes as u32 }

    pub(in super) fn modified_u64(&self) -> u64 {
        to_u64(&self.last_write_time)
    }

    pub(in super) fn accessed_u64(&self) -> u64 {
        to_u64(&self.last_access_time)
    }

    pub(in super) fn created_u64(&self) -> u64 {
        to_u64(&self.creation_time)
    }

    fn is_reparse_point(&self) -> bool {
        self.attributes & c::FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
}

fn to_u64(ft: &c::FILETIME) -> u64 {
    (ft.dwLowDateTime as u64) | ((ft.dwHighDateTime as u64) << 32)
}

impl FileTypePal for FileType {
    fn is_dir(&self) -> bool {
        *self == FileType::Dir
    }

    fn is_file(&self) -> bool {
        *self == FileType::File
    }

    fn is_symlink(&self) -> bool {
        *self == FileType::SymlinkFile ||
        *self == FileType::SymlinkDir ||
        *self == FileType::MountPoint
    }
}

impl FileType {
    fn new(attrs: c::DWORD, reparse_tag: c::DWORD) -> FileType {
        match (attrs & c::FILE_ATTRIBUTE_DIRECTORY != 0,
               attrs & c::FILE_ATTRIBUTE_REPARSE_POINT != 0,
               reparse_tag) {
            (false, false, _) => FileType::File,
            (true, false, _) => FileType::Dir,
            (false, true, c::IO_REPARSE_TAG_SYMLINK) => FileType::SymlinkFile,
            (true, true, c::IO_REPARSE_TAG_SYMLINK) => FileType::SymlinkDir,
            (true, true, c::IO_REPARSE_TAG_MOUNT_POINT) => FileType::MountPoint,
            (_, true, _) => FileType::ReparsePoint,
            // Note: if a _file_ has a reparse tag of the type IO_REPARSE_TAG_MOUNT_POINT it is
            // invalid, as junctions always have to be dirs. We set the filetype to ReparsePoint
            // to indicate it is something symlink-like, but not something you can follow.
        }
    }

    fn is_symlink_dir(&self) -> bool {
        *self == FileType::SymlinkDir || *self == FileType::MountPoint
    }
}

fn remove_dir_all_recursive(path: &Path) -> io::Result<()> {
    for child in Fs::read_dir(path)? {
        let child = child?;
        let child_type = child.file_type()?;
        if child_type.is_dir() {
            remove_dir_all_recursive(&child.path())?;
        } else if child_type.is_symlink_dir() {
            Fs::remove_dir(&child.path())?;
        } else {
            Fs::remove_file(&child.path())?;
        }
    }
    Fs::remove_dir(path)
}

pub(in super) fn symlink_inner(src: &Path, dst: &Path, dir: bool) -> io::Result<()> {
    let src = to_u16s(src)?;
    let dst = to_u16s(dst)?;
    let flags = if dir { c::SYMBOLIC_LINK_FLAG_DIRECTORY } else { 0 };
    // Formerly, symlink creation required the SeCreateSymbolicLink privilege. For the Windows 10
    // Creators Update, Microsoft loosened this to allow unprivileged symlink creation if the
    // computer is in Developer Mode, but SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE must be
    // added to dwFlags to opt into this behaviour.
    let result = cvt(unsafe {
        c::CreateSymbolicLinkW(dst.as_ptr(), src.as_ptr(),
                               flags | c::SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE) as c::BOOL
    });
    if let Err(err) = result {
        if err.raw_os_error() == Some(c::ERROR_INVALID_PARAMETER as i32) {
            // Older Windows objects to SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE,
            // so if we encounter ERROR_INVALID_PARAMETER, retry without that flag.
            cvt(unsafe {
                c::CreateSymbolicLinkW(dst.as_ptr(), src.as_ptr(), flags) as c::BOOL
            })?;
        } else {
            return Err(err);
        }
    }
    Ok(())
}

fn get_path(f: &File) -> io::Result<PathBuf> {
    super::fill_utf16_buf(|buf, sz| unsafe {
        c::GetFinalPathNameByHandleW(f.handle.raw(), buf, sz,
                                     c::VOLUME_NAME_DOS)
    }, |buf| {
        PathBuf::from(OsString::from_wide(buf))
    })
}

#[allow(dead_code)]
fn symlink_junction<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> io::Result<()> {
    symlink_junction_inner(src.as_ref(), dst.as_ref())
}

// Creating a directory junction on windows involves dealing with reparse
// points and the DeviceIoControl function, and this code is a skeleton of
// what can be found here:
//
// http://www.flexhex.com/docs/articles/hard-links.phtml
#[allow(dead_code)]
fn symlink_junction_inner(target: &Path, junction: &Path) -> io::Result<()> {
    let d = DirBuilder::new();
    d.create(&junction)?;

    let mut opts = OpenOptions::new();
    opts.write(true);
    opts.custom_flags(c::FILE_FLAG_OPEN_REPARSE_POINT |
                      c::FILE_FLAG_BACKUP_SEMANTICS);
    let f = File::open(junction, &opts)?;
    let h = f.handle().raw();

    unsafe {
        let mut data = [0u8; c::MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
        let db = data.as_mut_ptr()
                    as *mut c::REPARSE_MOUNTPOINT_DATA_BUFFER;
        let buf = &mut (*db).ReparseTarget as *mut _;
        let mut i = 0;
        // FIXME: this conversion is very hacky
        let v = br"\??\";
        let v = v.iter().map(|x| *x as u16);
        for c in v.chain(target.as_os_str().encode_wide()) {
            *buf.offset(i) = c;
            i += 1;
        }
        *buf.offset(i) = 0;
        i += 1;
        (*db).ReparseTag = c::IO_REPARSE_TAG_MOUNT_POINT;
        (*db).ReparseTargetMaximumLength = (i * 2) as c::WORD;
        (*db).ReparseTargetLength = ((i - 1) * 2) as c::WORD;
        (*db).ReparseDataLength =
                (*db).ReparseTargetLength as c::DWORD + 12;

        let mut ret = 0;
        cvt(c::DeviceIoControl(h as *mut _,
                               c::FSCTL_SET_REPARSE_POINT,
                               data.as_ptr() as *mut _,
                               (*db).ReparseDataLength + 8,
                               ptr::null_mut(), 0,
                               &mut ret,
                               ptr::null_mut())).map(|_| ())
    }
}
