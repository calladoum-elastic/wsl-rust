//! Registration-free WinRT activation of the `Microsoft.WSL.Containers.*` classes.
//!
//! The classes live in `wslcsdk.dll` and are mapped to it by a side-by-side manifest. Both are
//! embedded in the executable as resources by this crate's build script; [`init`] extracts
//! them next to the executable and installs the manifest as the process-default activation
//! context. This is the only `unsafe` code outside the bindings.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;

use windows_core::{Error, HRESULT, Result};

const ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID: u32 = 0x4;
const ACTCTX_FLAG_SET_PROCESS_DEFAULT: u32 = 0x10;
const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x2;
const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;
const INVALID_HANDLE_VALUE: isize = -1;
const ERROR_SXS_PROCESS_DEFAULT_ALREADY_SET: i32 = 14011;
const E_FAIL: HRESULT = HRESULT(0x8000_4005_u32 as i32);
const RT_RCDATA: u16 = 10;

/// Resource IDs of the embedded files; must match the `.rc` written by build.rs.
const DLL_RESOURCE_ID: u16 = 101;
const MANIFEST_RESOURCE_ID: u16 = 102;

/// Ref https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-actctxw
#[repr(C)]
struct ACTCTXW {
    cb_size: u32,
    flags: u32,
    source: *const u16,
    processor_architecture: u16,
    lang_id: u16,
    assembly_directory: *const u16,
    resource_name: *const u16,
    application_name: *const u16,
    module: *mut c_void,
}

windows_link::link!("kernel32.dll" "system" fn CreateActCtxW(actctx: *const ACTCTXW) -> isize);
windows_link::link!("kernel32.dll" "system" fn GetLastError() -> u32);
windows_link::link!("kernel32.dll" "system" fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut *mut c_void) -> i32);
windows_link::link!("kernel32.dll" "system" fn FindResourceW(module: *mut c_void, name: *const u16, ty: *const u16) -> *mut c_void);
windows_link::link!("kernel32.dll" "system" fn SizeofResource(module: *mut c_void, info: *mut c_void) -> u32);
windows_link::link!("kernel32.dll" "system" fn LoadResource(module: *mut c_void, info: *mut c_void) -> *mut c_void);
windows_link::link!("kernel32.dll" "system" fn LockResource(data: *mut c_void) -> *const c_void);

static INIT: OnceLock<std::result::Result<(), (HRESULT, String)>> = OnceLock::new();

/// Makes the `Microsoft.WSL.Containers` classes activatable in this process.
///
/// This function can be called many times without performance impact, but must be invoked at least once
/// before interacting with the other `wsl_sys` bindings
pub fn init() -> Result<()> {
    INIT.get_or_init(|| activate().map_err(|e| (e.code(), e.message())))
        .clone()
        .map_err(|(code, message)| Error::new(code, message))
}

/// HRESULT_FROM_WIN32
fn win32(code: u32) -> HRESULT {
    HRESULT((0x8007_0000 | (code & 0xFFFF)) as i32)
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

fn resource(id: u16) -> Result<&'static [u8]> {
    // MAKEINTRESOURCEW: integer IDs passed in place of string pointers.
    let (name, ty) = (id as usize as *const u16, RT_RCDATA as usize as *const u16);
    let missing = || {
        Error::new(
            Error::from_thread().code(),
            format!("resource {id} (wsl-sys's embedded wslcsdk runtime) is missing from this module"),
        )
    };

    // SAFETY: the address passed is a function in this module, which can't be unloaded while
    // its code runs, so the module handle and the resource memory it owns live for `'static`.
    // A resource found by FindResourceW is `size` bytes long.
    unsafe {
        let mut module = std::ptr::null_mut();
        let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
        if GetModuleHandleExW(flags, resource as *const u16, &mut module) == 0 {
            return Err(Error::from_thread());
        }
        let info = FindResourceW(module, name, ty);
        if info.is_null() {
            return Err(missing());
        }
        let size = SizeofResource(module, info);
        let data = LockResource(LoadResource(module, info));
        if data.is_null() {
            return Err(missing());
        }
        Ok(std::slice::from_raw_parts(data.cast(), size as usize))
    }
}

fn extract(path: &Path, contents: &[u8]) -> Result<()> {
    let up_to_date = || {
        std::fs::metadata(path).is_ok_and(|m| m.len() == contents.len() as u64)
            && std::fs::read(path).is_ok_and(|existing| existing == contents)
    };
    if up_to_date() {
        return Ok(());
    }

    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, contents).and_then(|()| std::fs::rename(&tmp, path));
    let _ = std::fs::remove_file(&tmp);
    match result {
        Ok(()) => Ok(()),
        // Another process may have extracted the same file meanwhile, and be using it.
        Err(_) if up_to_date() => Ok(()),
        Err(e) => Err(Error::new(
            e.raw_os_error().map_or(E_FAIL, |code| win32(code as u32)),
            format!("couldn't extract {}: {e}", path.display()),
        )),
    }
}

fn activate() -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    let dir = exe
        .parent()
        .ok_or_else(|| Error::new(E_FAIL, "the executable has no parent directory"))?;
    let manifest_path = dir.join("wslcsdk.manifest");
    extract(&dir.join("wslcsdk.dll"), resource(DLL_RESOURCE_ID)?)?;
    extract(&manifest_path, resource(MANIFEST_RESOURCE_ID)?)?;

    let manifest = wide(&manifest_path);
    let assembly_dir = wide(dir);
    let ctx = ACTCTXW {
        cb_size: size_of::<ACTCTXW>() as u32,
        flags: ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID | ACTCTX_FLAG_SET_PROCESS_DEFAULT,
        source: manifest.as_ptr(),
        processor_architecture: 0,
        lang_id: 0,
        assembly_directory: assembly_dir.as_ptr(),
        resource_name: std::ptr::null(),
        application_name: std::ptr::null(),
        module: std::ptr::null_mut(),
    };

    // SAFETY: `ctx` is fully initialised and its string pointers outlive the call.
    let handle = unsafe { CreateActCtxW(&ctx) };
    if handle == INVALID_HANDLE_VALUE {
        // SAFETY: trivially safe.
        let err = unsafe { GetLastError() };
        if err as i32 == ERROR_SXS_PROCESS_DEFAULT_ALREADY_SET {
            return Ok(());
        }
        return Err(Error::from_hresult(win32(err)));
    }
    // Leak context, so it's tied toprocess lifetime
    Ok(())
}
