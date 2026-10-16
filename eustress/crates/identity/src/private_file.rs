//! Files and folders only the signed-in user can open.
//!
//! For secrets the engine writes to disk (the bridge token). Nothing is left
//! to inheritance: a folder or file under the user's profile can inherit
//! access for other accounts (on one machine `%LOCALAPPDATA%` and
//! `~/.eustress_engine` grant read to a sandbox group an AI agent runs as),
//! so every folder and file this writes gets its own list, checked after it
//! is set, and a secret is written only once the check passes.
//!
//! - Windows: owned by the current user, with a protected access list
//!   (inheritance off) of two entries, the current user and SYSTEM. [`verify`]
//!   reads both back and fails, naming the account, if another account owns
//!   it (an owner can always rewrite the list) or any other entry is there.
//! - Unix: owned by the effective user, folders 0700, files 0600; [`verify`]
//!   fails on another owner or any group or other bit.
//!
//! [`verify_open`] opens a file and checks it through the open handle, so the
//! file checked is the file read, with no window between them.

use std::io;
use std::path::Path;

/// Create `dir` (and its parents) and make it a folder only the current
/// user (and, on Windows, SYSTEM) can open, then check that it is.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    imp::protect(dir, true)?;
    verify(dir)
}

/// Write `bytes` to `path` so that only the current user (and, on Windows,
/// SYSTEM) can read them. The folder is made private first; the file is
/// created empty, its access set and checked, and only then are the bytes
/// written, to a temporary name renamed into place, so a reader never sees
/// a half-written file and the bytes are never readable by anyone else.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a private file needs a folder"))?;
    private_dir(dir)?;
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = dir.join(tmp_name);
    let _ = std::fs::remove_file(&tmp);
    imp::create_empty(&tmp)?;
    imp::protect(&tmp, false)?;
    verify(&tmp)?;
    let mut f = std::fs::OpenOptions::new().write(true).truncate(true).open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)?;
    verify(path)
}

/// `Ok` when the current user owns `path` and only they (and, on Windows,
/// SYSTEM) can open it; otherwise an error that says who else owns or can
/// open it.
pub fn verify(path: &Path) -> io::Result<()> {
    imp::verify(path).map_err(|e| not_private(path, e))
}

/// Open the file at `path` for reading and check it as [`verify`] does,
/// through the open handle, so the file checked is the file read: nothing
/// can swap it in between. On Windows no one can write into the file while
/// the handle is open, but a rename may replace the path (as
/// [`write_private`] does): the handle keeps reading the file it checked, and
/// the next open reads the new one.
pub fn verify_open(path: &Path) -> io::Result<std::fs::File> {
    imp::verify_open(path).map_err(|e| not_private(path, e))
}

fn not_private(path: &Path, e: io::Error) -> io::Error {
    if let Some(owner) = not_owner(&e) {
        return NotOwner { path: path.display().to_string(), ..owner.clone() }.into();
    }
    io::Error::new(e.kind(), format!("{} is not private: {e}", path.display()))
}

/// Why [`verify`] refused a file or folder another account owns: the owner
/// can always rewrite its access list, so the list proves nothing. Carried
/// inside the `io::Error` ([`not_owner`] finds it), so a caller can tell this
/// from a wrong list and decide by who the owner is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotOwner {
    /// The file or folder, once [`verify`] has named it.
    pub path: String,
    /// The owner: a SID on Windows, a user id on Unix.
    pub owner: String,
    /// The owner is trusted on this machine: the Administrators group or
    /// SYSTEM on Windows (an elevated run of this user creates files owned
    /// by Administrators), root on Unix. Any other owner is another account.
    pub trusted: bool,
    /// How to take ownership back.
    pub advice: String,
}

impl std::fmt::Display for NotOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.path.is_empty() {
            write!(f, "{} is not private: ", self.path)?;
        }
        write!(f, "it is owned by {}, not the current user; {}", self.owner, self.advice)
    }
}

impl std::error::Error for NotOwner {}

impl From<NotOwner> for io::Error {
    fn from(e: NotOwner) -> Self {
        io::Error::new(io::ErrorKind::PermissionDenied, e)
    }
}

/// The [`NotOwner`] inside an error [`verify`] or [`verify_open`] returned,
/// if that is why it failed.
pub fn not_owner(e: &io::Error) -> Option<&NotOwner> {
    e.get_ref()?.downcast_ref::<NotOwner>()
}

#[cfg(unix)]
mod imp {
    use std::io;
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    use std::path::Path;

    pub fn protect(path: &Path, dir: bool) -> io::Result<()> {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(if dir { 0o700 } else { 0o600 }))
    }

    pub fn create_empty(path: &Path) -> io::Result<()> {
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map(drop)
    }

    pub fn verify(path: &Path) -> io::Result<()> {
        check(&std::fs::metadata(path)?)
    }

    pub fn verify_open(path: &Path) -> io::Result<std::fs::File> {
        let file = std::fs::File::open(path)?;
        // fstat: the open file's own owner and mode.
        check(&file.metadata()?)?;
        Ok(file)
    }

    fn check(meta: &std::fs::Metadata) -> io::Result<()> {
        use std::os::unix::fs::MetadataExt as _;
        owner_and_mode(meta.uid(), meta.mode(), effective_uid())
    }

    /// Owned by `euid`, and no group or other bit.
    pub(super) fn owner_and_mode(uid: u32, mode: u32, euid: u32) -> io::Result<()> {
        if uid != euid {
            return Err(super::NotOwner {
                path: String::new(),
                owner: format!("user {uid}"),
                trusted: uid == 0,
                advice: format!("take ownership with: chown {euid} <file>"),
            }
            .into());
        }
        if mode & 0o077 != 0 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("its mode is {:o}", mode & 0o777)));
        }
        Ok(())
    }

    pub(super) fn effective_uid() -> u32 {
        extern "C" {
            fn geteuid() -> u32;
        }
        // SAFETY: geteuid takes nothing, cannot fail and touches no memory.
        unsafe { geteuid() }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::Path;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
        GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
        GetTokenInformation, IsWellKnownSid, SetFileSecurityW, TokenUser, WinLocalSystemSid, ACCESS_ALLOWED_ACE,
        ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER, WinBuiltinAdministratorsSid,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn other(message: String) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, message)
    }

    /// The current user's SID, in a buffer that owns it (u64s, so the
    /// `TOKEN_USER` at its start is aligned).
    pub(super) struct UserSid(Vec<u64>);

    impl UserSid {
        pub(super) fn current() -> io::Result<Self> {
            // SAFETY: the token handle is closed on every path; the buffer is
            // sized by the first call and outlives every read of it.
            unsafe {
                let mut token: HANDLE = null_mut();
                if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let mut len = 0u32;
                GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
                let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
                let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut len);
                let err = io::Error::last_os_error();
                CloseHandle(token);
                if ok == 0 {
                    return Err(err);
                }
                Ok(Self(buf))
            }
        }

        pub(super) fn psid(&self) -> PSID {
            // SAFETY: the buffer holds the TOKEN_USER GetTokenInformation wrote.
            unsafe { (*(self.0.as_ptr() as *const TOKEN_USER)).User.Sid }
        }
    }

    /// `S-1-5-...` for a SID.
    pub(super) fn sid_string(sid: PSID) -> String {
        // SAFETY: ConvertSidToStringSidW allocates a NUL-terminated string,
        // freed with LocalFree after it is copied.
        unsafe {
            let mut s: windows_sys::core::PWSTR = null_mut();
            if ConvertSidToStringSidW(sid, &mut s) == 0 || s.is_null() {
                return "an account whose SID cannot be read".to_string();
            }
            let mut n = 0;
            while *s.add(n) != 0 {
                n += 1;
            }
            let out = String::from_utf16_lossy(std::slice::from_raw_parts(s, n));
            LocalFree(s.cast());
            out
        }
    }

    /// Make the current user `path`'s owner, and replace its access list with
    /// one that does not inherit and names only the current user and SYSTEM,
    /// each with full access. (The owner is set, not assumed: an elevated
    /// process can create files owned by the Administrators group.)
    pub fn protect(path: &Path, dir: bool) -> io::Result<()> {
        let user = UserSid::current()?;
        let me = sid_string(user.psid());
        let inherit = if dir { "OICI" } else { "" };
        let sddl = format!("O:{me}D:P(A;{inherit};FA;;;{me})(A;{inherit};FA;;;SY)");
        let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        // SAFETY: the descriptor is allocated by the conversion and freed
        // with LocalFree after SetFileSecurityW has copied it.
        unsafe {
            let mut sd: PSECURITY_DESCRIPTOR = null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, null_mut()) == 0 {
                return Err(io::Error::last_os_error());
            }
            let ok = SetFileSecurityW(
                wide(path).as_ptr(),
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                sd,
            );
            let err = io::Error::last_os_error();
            LocalFree(sd);
            if ok == 0 {
                return Err(err);
            }
        }
        Ok(())
    }

    pub fn create_empty(path: &Path) -> io::Result<()> {
        std::fs::OpenOptions::new().write(true).create_new(true).open(path).map(drop)
    }

    pub fn verify(path: &Path) -> io::Result<()> {
        let user = UserSid::current()?;
        // SAFETY: GetNamedSecurityInfoW allocates the descriptor; it is freed
        // once the check has finished reading it.
        unsafe {
            let mut sd: PSECURITY_DESCRIPTOR = null_mut();
            let rc = GetNamedSecurityInfoW(
                wide(path).as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut sd,
            );
            if rc != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(rc as i32));
            }
            let result = check(sd, &user, path);
            LocalFree(sd);
            result
        }
    }

    pub fn verify_open(path: &Path) -> io::Result<std::fs::File> {
        use std::os::windows::fs::OpenOptionsExt as _;
        use std::os::windows::io::AsRawHandle as _;
        let user = UserSid::current()?;
        // Readers and a rename over the path, never a writer: write_private's
        // replace must not fail while this is open, and the bytes being read
        // must not change under it.
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_DELETE: u32 = 4;
        let file = std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE).open(path)?;
        // SAFETY: the handle is the open file's; GetSecurityInfo allocates
        // the descriptor, freed once the check has read it.
        unsafe {
            let mut sd: PSECURITY_DESCRIPTOR = null_mut();
            let rc = GetSecurityInfo(
                file.as_raw_handle() as HANDLE,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut sd,
            );
            if rc != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(rc as i32));
            }
            let result = check(sd, &user, path);
            LocalFree(sd);
            result?;
        }
        Ok(file)
    }

    /// The user owns it, the list is protected, and every entry is the
    /// user's or SYSTEM's. `path` only names it in the ownership advice.
    pub(super) unsafe fn check(sd: PSECURITY_DESCRIPTOR, user: &UserSid, path: &Path) -> io::Result<()> {
        // The owner can always rewrite the list (implicit WRITE_DAC), so a
        // list that is right today proves nothing if another account owns it.
        let mut owner: PSID = null_mut();
        let mut defaulted = 0;
        if GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted) == 0 {
            return Err(io::Error::last_os_error());
        }
        if owner.is_null() {
            return Err(other("it has no owner recorded".to_string()));
        }
        if EqualSid(owner, user.psid()) == 0 {
            return Err(super::NotOwner {
                path: String::new(),
                owner: sid_string(owner),
                trusted: IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0
                    || IsWellKnownSid(owner, WinLocalSystemSid) != 0,
                advice: format!(
                    "the current user is {}; take ownership with: icacls \"{}\" /setowner \"%USERNAME%\"",
                    sid_string(user.psid()),
                    path.display()
                ),
            }
            .into());
        }
        let mut control = 0u16;
        let mut revision = 0u32;
        if GetSecurityDescriptorControl(sd, &mut control, &mut revision) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut present = 0;
        let mut dacl: *mut ACL = null_mut();
        if GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) == 0 {
            return Err(io::Error::last_os_error());
        }
        if present == 0 {
            dacl = null_mut();
        }
        if control & SE_DACL_PROTECTED == 0 {
            return Err(other("it inherits access from its folder".to_string()));
        }
        if dacl.is_null() {
            return Err(other("it has no access list, so every account can open it".to_string()));
        }
        for i in 0..u32::from((*dacl).AceCount) {
            let mut ace: *mut c_void = null_mut();
            if GetAce(dacl, i, &mut ace) == 0 {
                return Err(io::Error::last_os_error());
            }
            let header = &*(ace as *const ACE_HEADER);
            // Allowed (0) and denied (1) entries keep their SID where
            // ACCESS_ALLOWED_ACE's SidStart is; any other kind is refused.
            if header.AceType > 1 {
                return Err(other(format!("it has an access entry of kind {}", header.AceType)));
            }
            let sid: PSID = (ace as *mut u8).add(std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart)).cast();
            if EqualSid(sid, user.psid()) == 0 && IsWellKnownSid(sid, WinLocalSystemSid) == 0 {
                return Err(other(format!("it also gives access to {}", sid_string(sid))));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eus_private_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A file written the ordinary way takes its folder's access, and the
    /// check says so: the check can fail, so its passing below means
    /// something.
    #[test]
    fn an_ordinary_file_is_not_private() {
        let dir = scratch("plain");
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("plain.txt");
        std::fs::write(&plain, "x").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&plain, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
        assert!(verify(&plain).is_err(), "an inherited or world-readable file must fail the check");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A private file, in a private folder made inside a folder that may
    /// grant other accounts access, reads back whole and passes the check,
    /// as does its folder; no temporary file is left.
    #[test]
    fn a_private_file_passes_and_reads_back() {
        let dir = scratch("secret");
        let path = dir.join("inner").join("50123.token");
        write_private(&path, b"abc123").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abc123");
        verify(&path).unwrap();
        verify(path.parent().unwrap()).unwrap();
        assert!(!path.with_file_name("50123.token.tmp").exists());
        write_private(&path, b"def456").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"def456", "a second write replaces the first");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// verify_open returns the checked file, open, and refuses one that is
    /// not private.
    /// While a verify_open handle is held, write_private can replace the file
    /// (a revocation must not fail), the held handle still reads what it
    /// checked, and nobody can open the held file for writing.
    #[cfg(windows)]
    #[test]
    fn a_held_verify_open_handle_allows_a_replace_but_no_writer() {
        use std::io::Read as _;
        let dir = scratch("share");
        let path = dir.join("parties.toml");
        write_private(&path, b"old").unwrap();
        let mut held = verify_open(&path).unwrap();
        assert!(
            std::fs::OpenOptions::new().write(true).open(&path).is_err(),
            "a writer opened the file a verify_open handle is reading"
        );
        write_private(&path, b"new").expect("write_private must replace the file while a reader holds it");
        let mut text = String::new();
        held.read_to_string(&mut text).unwrap();
        assert_eq!(text, "old", "the held handle reads the file it checked");
        drop(held);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_open_reads_the_file_it_checked() {
        use std::io::Read as _;
        let dir = scratch("open");
        let path = dir.join("parties.toml");
        write_private(&path, b"admin = []").unwrap();
        let mut text = String::new();
        verify_open(&path).unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "admin = []");
        let plain = dir.join("plain.toml");
        std::fs::write(&plain, "x").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&plain, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
        assert!(verify_open(&plain).is_err(), "an ordinary file must be refused");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Another account as owner fails, even with a list that is otherwise
    /// right: the owner could rewrite the list at any time. The descriptor is
    /// built in memory (setting another account as a file's owner needs a
    /// privilege tests do not have), and the check is the one verify uses.
    #[cfg(windows)]
    #[test]
    fn another_owner_fails_even_with_a_private_list() {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::PSECURITY_DESCRIPTOR;
        let user = imp::UserSid::current().unwrap();
        let me = imp::sid_string(user.psid());
        let check = |owner: &str| {
            let sddl: Vec<u16> = format!("O:{owner}D:P(A;;FA;;;{me})(A;;FA;;;SY)").encode_utf16().chain(Some(0)).collect();
            // SAFETY: the descriptor is made by the conversion and freed
            // after the check has read it.
            unsafe {
                let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
                assert_ne!(
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut()),
                    0
                );
                let result = imp::check(sd, &user, Path::new("C:\\x\\parties.toml"));
                LocalFree(sd);
                result
            }
        };
        check(&me).expect("owned by the user, private list: passes");
        for (owner, trusted) in [("SY", true), ("BA", true), ("S-1-5-21-1-2-3-1001", false)] {
            let err = check(owner).expect_err(owner);
            assert!(err.to_string().contains("/setowner"), "{err}");
            let found = not_owner(&err).unwrap_or_else(|| panic!("{owner}: not a NotOwner error: {err}"));
            assert_eq!(found.trusted, trusted, "{owner}");
        }
        // The kind survives verify naming the path.
        let named = not_private(Path::new("C:\\x\\auth_token"), check("S-1-5-21-1-2-3-1001").unwrap_err());
        let found = not_owner(&named).expect("still a NotOwner");
        assert!(!found.trusted);
        assert!(named.to_string().starts_with("C:\\x\\auth_token is not private: it is owned by S-1-5-21-1-2-3-1001"), "{named}");
    }

    /// A file write_private made is owned by the current user.
    #[cfg(windows)]
    #[test]
    fn write_private_makes_the_user_the_owner() {
        let dir = scratch("owner");
        let path = dir.join("k.key");
        write_private(&path, b"k").unwrap();
        verify(&path).unwrap();
        verify(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Another owner fails the Unix check, private mode or not.
    #[cfg(unix)]
    #[test]
    fn another_owner_fails_on_unix() {
        let me = imp::effective_uid();
        assert!(imp::owner_and_mode(me, 0o600, me).is_ok());
        let other = imp::owner_and_mode(me + 1, 0o600, me).expect_err("another owner");
        assert_eq!(not_owner(&other).map(|n| n.trusted), Some(me + 1 == 0));
        assert_eq!(not_owner(&imp::owner_and_mode(0, 0o600, 1000).unwrap_err()).map(|n| n.trusted), Some(true), "root");
        assert!(imp::owner_and_mode(me, 0o640, me).is_err(), "a group bit");
    }

    /// With the rights to chown (root), a private file given to another user
    /// fails verify.
    #[cfg(unix)]
    #[test]
    fn a_chowned_file_fails_when_root() {
        if imp::effective_uid() != 0 {
            return;
        }
        let dir = scratch("chown");
        let path = dir.join("p.toml");
        write_private(&path, b"x").unwrap();
        std::os::unix::fs::chown(&path, Some(65534), None).unwrap();
        assert!(verify(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
