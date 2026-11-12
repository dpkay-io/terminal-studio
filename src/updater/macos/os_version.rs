//! The running macOS version, read without starting a process.

use std::sync::OnceLock;

/// sysctl name for the product version ("27.2"); NUL-terminated for C.
const PRODUCT_VERSION_KEY: &[u8] = b"kern.osproductversion\0";
/// Room for any "major.minor.patch" string.
const VERSION_BUF_LEN: usize = 32;
const VERSION_SEPARATOR: char = '.';

/// The major version of the running macOS (27 for 27.2), or `None` when it
/// cannot be read. Read once; the OS does not change under a running app.
pub(super) fn major() -> Option<u32> {
    static MAJOR: OnceLock<Option<u32>> = OnceLock::new();
    *MAJOR.get_or_init(|| product_version().as_deref().and_then(major_of))
}

fn product_version() -> Option<String> {
    let mut buf = [0u8; VERSION_BUF_LEN];
    let mut len = buf.len();
    // SAFETY: the key is NUL-terminated, and `buf`/`len` describe a writable
    // buffer that sysctl fills up to `len` bytes (failing if it is too small).
    let status = unsafe {
        libc::sysctlbyname(
            PRODUCT_VERSION_KEY.as_ptr().cast(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    let bytes = buf.get(..len)?;
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

fn major_of(version: &str) -> Option<u32> {
    version.trim().split(VERSION_SEPARATOR).next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The oldest macOS the app runs on (`LSMinimumSystemVersion`).
    const MIN_SUPPORTED_MAJOR: u32 = 11;

    #[test]
    fn major_of_reads_the_leading_number() {
        assert_eq!(major_of("27.2"), Some(27));
        assert_eq!(major_of("11.7.10"), Some(11));
        assert_eq!(major_of("13"), Some(13));
    }

    #[test]
    fn major_of_rejects_unreadable_versions() {
        assert_eq!(major_of(""), None);
        assert_eq!(major_of("abc"), None);
        assert_eq!(major_of(".2"), None);
    }

    #[test]
    fn major_reads_this_mac() {
        let major = major().expect("kern.osproductversion is readable");
        assert!(major >= MIN_SUPPORTED_MAJOR, "{major}");
    }
}
