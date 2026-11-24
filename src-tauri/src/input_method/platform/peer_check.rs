//! DanyWhere: who is at the other end of the link (PROTOCOL.md, "The
//! link"): the peer's audit token (`LOCAL_PEERTOKEN`) → its code
//! (`SecCodeCopyGuestWithAttributes`) → checked against the requirement
//! `identifier "<input method id>" and certificate leaf = H"<SHA-1 of the
//! app's own leaf certificate>"`. App and input method are always signed
//! with the same certificate; an app whose own code has none (ad hoc,
//! unsigned) lets no peer in.

use std::ffi::c_void;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::ptr::null;

use super::cf::{self, CFDataRef, CFDictionaryRef, CFStringRef, Owned};
use super::link::PeerCheck;

type OSStatus = i32;
type SecCodeRef = *const c_void;
type SecStaticCodeRef = *const c_void;
type SecRequirementRef = *const c_void;
type SecCertificateRef = *const c_void;

const DEFAULT_FLAGS: u32 = 0;
/// `kSecCSSigningInformation`: include the certificates.
const SIGNING_INFORMATION: u32 = 1 << 1;
/// `sizeof(audit_token_t)`.
const AUDIT_TOKEN_SIZE: usize = 32;

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecCodeCopySelf(flags: u32, code: *mut SecCodeRef) -> OSStatus;
    fn SecCodeCopyStaticCode(
        code: SecCodeRef,
        flags: u32,
        static_code: *mut SecStaticCodeRef,
    ) -> OSStatus;
    fn SecCodeCopySigningInformation(
        code: SecStaticCodeRef,
        flags: u32,
        information: *mut CFDictionaryRef,
    ) -> OSStatus;
    fn SecCodeCopyGuestWithAttributes(
        host: SecCodeRef,
        attributes: CFDictionaryRef,
        flags: u32,
        guest: *mut SecCodeRef,
    ) -> OSStatus;
    fn SecRequirementCreateWithString(
        text: CFStringRef,
        flags: u32,
        requirement: *mut SecRequirementRef,
    ) -> OSStatus;
    fn SecCodeCheckValidity(
        code: SecCodeRef,
        flags: u32,
        requirement: SecRequirementRef,
    ) -> OSStatus;
    fn SecCertificateCopyData(certificate: SecCertificateRef) -> CFDataRef;
    static kSecGuestAttributeAudit: CFStringRef;
    static kSecCodeInfoCertificates: CFStringRef;
}

// CommonCrypto, part of libSystem.
extern "C" {
    fn CC_SHA1(data: *const c_void, len: u32, md: *mut u8) -> *mut u8;
}

/// The code requirement a peer must meet.
pub fn requirement_text(identifier: &str, leaf_sha1_hex: &str) -> String {
    format!("identifier \"{identifier}\" and certificate leaf = H\"{leaf_sha1_hex}\"")
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut digest = [0u8; 20];
    // SAFETY: `bytes` is valid for its length; the digest buffer holds the
    // 20 bytes CC_SHA1 writes.
    unsafe {
        CC_SHA1(
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            digest.as_mut_ptr(),
        )
    };
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The SHA-1 of this process's own leaf certificate in hex, or `None` when
/// its code has no certificate (ad hoc or unsigned).
pub fn own_leaf_certificate_sha1() -> Option<String> {
    // SAFETY: every out-pointer is a valid local; every returned object
    // follows the Create/Copy rule and is released by `Owned`; borrowed
    // values (the certificates array and its leaf) are used while `info`
    // keeps them alive.
    unsafe {
        let mut code: SecCodeRef = null();
        if SecCodeCopySelf(DEFAULT_FLAGS, &mut code) != 0 {
            return None;
        }
        let code = Owned::take(code)?;
        let mut static_code: SecStaticCodeRef = null();
        if SecCodeCopyStaticCode(code.as_ptr(), DEFAULT_FLAGS, &mut static_code) != 0 {
            return None;
        }
        let static_code = Owned::take(static_code)?;
        let mut info: CFDictionaryRef = null();
        if SecCodeCopySigningInformation(static_code.as_ptr(), SIGNING_INFORMATION, &mut info) != 0
        {
            return None;
        }
        let info = Owned::take(info)?;
        let certificates = cf::dictionary_get(info.as_ptr(), kSecCodeInfoCertificates);
        if certificates.is_null() {
            return None;
        }
        let leaf = *cf::array_items(certificates).first()?;
        let der = Owned::take(SecCertificateCopyData(leaf))?;
        let bytes = cf::data_bytes(der.as_ptr());
        (!bytes.is_empty()).then(|| sha1_hex(&bytes))
    }
}

/// The audit token of the process at the other end of `stream`.
fn peer_audit_token(stream: &UnixStream) -> io::Result<[u8; AUDIT_TOKEN_SIZE]> {
    let mut token = [0u8; AUDIT_TOKEN_SIZE];
    let mut len = AUDIT_TOKEN_SIZE as libc::socklen_t;
    // SAFETY: the buffer is valid for `len` bytes, which getsockopt updates.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != AUDIT_TOKEN_SIZE {
        return Err(io::Error::other(format!("a token of {len} bytes")));
    }
    Ok(token)
}

/// Checks the code of the process with `token` against `requirement`.
fn check_token(token: &[u8], requirement: &str) -> Result<(), String> {
    let data = cf::data(token).ok_or("the audit token couldn't be passed on")?;
    // SAFETY: the attribute key is a constant of Security; `data` lives
    // until the dictionary (which retains it) is built.
    let attributes = cf::dictionary(&[(unsafe { kSecGuestAttributeAudit }, data.as_ptr())])
        .ok_or("the audit token couldn't be passed on")?;
    let text = cf::string(requirement).ok_or("the requirement couldn't be passed on")?;
    // SAFETY: valid CF objects in, owned objects out (released by `Owned`).
    unsafe {
        let mut guest: SecCodeRef = null();
        let status =
            SecCodeCopyGuestWithAttributes(null(), attributes.as_ptr(), DEFAULT_FLAGS, &mut guest);
        if status != 0 {
            return Err(format!("the peer's code is unknown (OSStatus {status})"));
        }
        let guest = Owned::take(guest).ok_or("the peer's code is unknown")?;
        let mut compiled: SecRequirementRef = null();
        let status = SecRequirementCreateWithString(text.as_ptr(), DEFAULT_FLAGS, &mut compiled);
        if status != 0 {
            return Err(format!("the requirement is invalid (OSStatus {status})"));
        }
        let compiled = Owned::take(compiled).ok_or("the requirement is invalid")?;
        let status = SecCodeCheckValidity(guest.as_ptr(), DEFAULT_FLAGS, compiled.as_ptr());
        if status != 0 {
            return Err(format!(
                "the peer doesn't meet the requirement (OSStatus {status})"
            ));
        }
    }
    Ok(())
}

/// The real check, by code signature.
pub struct CodeSignaturePeerCheck {
    requirement: Option<String>,
}

impl CodeSignaturePeerCheck {
    /// Expects a peer with `identifier`, signed with this app's own leaf
    /// certificate. Without one of its own, no peer gets in (logged here,
    /// once).
    pub fn for_peer(identifier: &str) -> Self {
        let requirement =
            own_leaf_certificate_sha1().map(|sha1| requirement_text(identifier, &sha1));
        match &requirement {
            Some(text) => log::info!("Input method link: a peer must meet: {text}"),
            None => log::warn!(
                "Input method link: this app's code has no certificate (ad hoc or \
                 unsigned), so no input method can link up"
            ),
        }
        CodeSignaturePeerCheck { requirement }
    }

    /// Expects a peer that meets `requirement` (code requirement language).
    #[cfg(test)]
    pub fn with_requirement(requirement: String) -> Self {
        CodeSignaturePeerCheck {
            requirement: Some(requirement),
        }
    }
}

impl PeerCheck for CodeSignaturePeerCheck {
    fn verify(&self, stream: &UnixStream) -> Result<(), String> {
        let Some(requirement) = &self.requirement else {
            return Err("this app's code has no certificate, so it trusts no peer".into());
        };
        let token = peer_audit_token(stream)
            .map_err(|e| format!("the peer's audit token is unknown: {e}"))?;
        check_token(&token, requirement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_requirement_names_the_id_and_the_certificate() {
        assert_eq!(
            requirement_text(
                "com.yaccob.danywhere.inputmethod.DanyWhere",
                "0123456789abcdef0123456789abcdef01234567"
            ),
            "identifier \"com.yaccob.danywhere.inputmethod.DanyWhere\" and \
             certificate leaf = H\"0123456789abcdef0123456789abcdef01234567\""
        );
    }

    #[test]
    fn sha1_is_the_usual_one() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    /// `cargo test`'s binary is signed ad hoc by the linker (or not at
    /// all): it has no certificate.
    #[test]
    fn the_test_binary_has_no_certificate() {
        assert_eq!(own_leaf_certificate_sha1(), None);
    }

    #[test]
    fn without_a_certificate_of_its_own_no_peer_gets_in() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let check = CodeSignaturePeerCheck::for_peer("com.yaccob.danywhere.inputmethod.DanyWhere");
        let refused = check.verify(&ours).expect_err("refused");
        assert!(refused.contains("certificate"), "{refused}");
    }

    /// The whole way through macOS for a peer that doesn't meet the
    /// requirement: this ad hoc test binary itself, at the other end of a
    /// socket pair. It is refused by the requirement, not by an earlier
    /// step.
    #[test]
    fn an_ad_hoc_peer_is_refused_by_the_requirement() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let check = CodeSignaturePeerCheck::with_requirement(requirement_text(
            "com.yaccob.danywhere.inputmethod.DanyWhere",
            "0000000000000000000000000000000000000000",
        ));
        let refused = check.verify(&ours).expect_err("refused");
        assert!(
            refused.starts_with("the peer doesn't meet the requirement"),
            "{refused}"
        );
    }
}
