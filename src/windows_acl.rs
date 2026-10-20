//! Restrict private state to the current Windows SID and LocalSystem.
use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        DACL_SECURITY_INFORMATION, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION,
        SetFileSecurityW, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn checked(result: i32) -> std::io::Result<()> {
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub fn protect(path: &Path) -> std::io::Result<()> {
    unsafe {
        let mut token = ptr::null_mut();
        checked(OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY,
            &mut token,
        ))?;
        let token = Handle(token);
        let mut size = 0;
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut size);
        if size == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // usize storage provides the alignment required by TOKEN_USER.
        let mut data = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        checked(GetTokenInformation(
            token.0,
            TokenUser,
            data.as_mut_ptr().cast(),
            size,
            &mut size,
        ))?;
        let user = &*data.as_ptr().cast::<TOKEN_USER>();
        let mut sid = ptr::null_mut();
        checked(ConvertSidToStringSidW(user.User.Sid, &mut sid))?;
        let _sid_memory = Local(sid.cast());
        let mut length = 0;
        while *sid.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(sid, length));
        let descriptor: Vec<u16> = format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut security = ptr::null_mut();
        checked(ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            1,
            &mut security,
            ptr::null_mut(),
        ))?;
        let _security_memory = Local(security);
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        checked(SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            security,
        ))
    }
}
