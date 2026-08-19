use std::os::raw::c_int;

pub mod ffi {
    use std::os::raw::c_int;

    pub type Handle = u64;

    pub const OK: c_int = 0;
    pub const ERR_UNSUPPORTED: c_int = -1;
    pub const ERR_SYSTEM: c_int = -2;
    pub const ERR_INVALID: c_int = -3;

    unsafe extern "C" {
        pub fn procguard_create(out: *mut Handle) -> c_int;
        pub fn procguard_prepare_child() -> c_int;
        pub fn procguard_adopt(handle: *mut Handle, pid: u64) -> c_int;
        pub fn procguard_terminate(handle: Handle) -> c_int;
        pub fn procguard_destroy(handle: Handle) -> c_int;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("process supervision is not supported on this platform")]
    Unsupported,

    #[error("the operating system refused a process supervision call")]
    System,

    #[error("a process supervision call was given an invalid argument")]
    Invalid,

    #[error("process supervision failed with an unrecognised code {0}")]
    Unknown(c_int),
}

pub type Result<T> = std::result::Result<T, Error>;

fn check(code: c_int) -> Result<()> {
    match code {
        ffi::OK => Ok(()),
        ffi::ERR_UNSUPPORTED => Err(Error::Unsupported),
        ffi::ERR_SYSTEM => Err(Error::System),
        ffi::ERR_INVALID => Err(Error::Invalid),
        other => Err(Error::Unknown(other)),
    }
}

#[derive(Debug)]
pub struct ProcessGuard {
    handle: ffi::Handle,
    adopted: bool,
}

impl ProcessGuard {
    pub fn new() -> Result<Self> {
        let mut handle: ffi::Handle = 0;
        check(unsafe { ffi::procguard_create(&mut handle) })?;
        Ok(Self {
            handle,
            adopted: false,
        })
    }

    pub fn adopt(&mut self, pid: u32) -> Result<()> {
        check(unsafe { ffi::procguard_adopt(&mut self.handle, u64::from(pid)) })?;
        self.adopted = true;
        Ok(())
    }

    pub fn terminate(&self) -> Result<()> {
        if !self.adopted {
            return Ok(());
        }
        check(unsafe { ffi::procguard_terminate(self.handle) })
    }

    pub fn is_guarding(&self) -> bool {
        self.adopted
    }

    /// # Safety
    ///
    /// Must only be called between `fork` and `exec`, where the caller is
    /// restricted to async-signal-safe operations.
    pub unsafe fn prepare_child() -> Result<()> {
        check(unsafe { ffi::procguard_prepare_child() })
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if self.adopted {
            unsafe { ffi::procguard_destroy(self.handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guard_can_be_created_and_dropped_without_a_child() {
        let guard = ProcessGuard::new().expect("a guard is created");
        assert!(!guard.is_guarding());
        assert!(guard.terminate().is_ok());
    }

    #[test]
    fn adopting_process_zero_is_rejected() {
        let mut guard = ProcessGuard::new().unwrap();
        assert_eq!(guard.adopt(0), Err(Error::Invalid));
        assert!(!guard.is_guarding());
    }

    #[test]
    fn error_codes_map_to_named_variants() {
        assert_eq!(check(ffi::OK), Ok(()));
        assert_eq!(check(ffi::ERR_UNSUPPORTED), Err(Error::Unsupported));
        assert_eq!(check(ffi::ERR_SYSTEM), Err(Error::System));
        assert_eq!(check(ffi::ERR_INVALID), Err(Error::Invalid));
        assert_eq!(check(-99), Err(Error::Unknown(-99)));
    }
}
