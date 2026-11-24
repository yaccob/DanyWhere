//! DanyWhere: the few Core Foundation calls the input source and peer
//! checks need, with ownership made explicit ([`Owned`] releases on drop).

use std::ffi::c_void;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFDataRef = *const c_void;
pub type CFURLRef = *const c_void;
pub type CFIndex = isize;
pub type Boolean = u8;

const UTF8: u32 = 0x0800_0100;

/// `CFDictionaryKeyCallBacks`: six pointer-sized fields, only ever passed
/// by address.
#[repr(C)]
pub struct DictionaryKeyCallBacks {
    _fields: [usize; 6],
}

/// `CFDictionaryValueCallBacks`: five pointer-sized fields.
#[repr(C)]
pub struct DictionaryValueCallBacks {
    _fields: [usize; 5],
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        len: CFIndex,
        encoding: u32,
        is_external: Boolean,
    ) -> CFStringRef;
    fn CFStringGetLength(string: CFStringRef) -> CFIndex;
    fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut std::ffi::c_char,
        size: CFIndex,
        encoding: u32,
    ) -> Boolean;
    fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        count: CFIndex,
        key_callbacks: *const DictionaryKeyCallBacks,
        value_callbacks: *const DictionaryValueCallBacks,
    ) -> CFDictionaryRef;
    fn CFDictionaryGetValue(dictionary: CFDictionaryRef, key: *const c_void) -> *const c_void;
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> *const c_void;
    fn CFDataCreate(alloc: *const c_void, bytes: *const u8, len: CFIndex) -> CFDataRef;
    fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    fn CFDataGetLength(data: CFDataRef) -> CFIndex;
    fn CFURLCreateFromFileSystemRepresentation(
        alloc: *const c_void,
        buffer: *const u8,
        len: CFIndex,
        is_directory: Boolean,
    ) -> CFURLRef;
    static kCFTypeDictionaryKeyCallBacks: DictionaryKeyCallBacks;
    static kCFTypeDictionaryValueCallBacks: DictionaryValueCallBacks;
}

/// A Core Foundation object this code owns (got by a Create or Copy
/// function), released on drop.
pub struct Owned(CFTypeRef);

impl Owned {
    /// Takes ownership of `object`; `None` for null.
    ///
    /// # Safety
    /// `object` must be null or a reference the caller owns (Create/Copy
    /// rule), released nowhere else.
    pub unsafe fn take(object: CFTypeRef) -> Option<Owned> {
        // Not `then_some(Owned(object))`: it builds the `Owned` first, and
        // dropping it for null would release null.
        if object.is_null() {
            None
        } else {
            Some(Owned(object))
        }
    }

    pub fn as_ptr(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: `take` only wraps non-null references this code owns.
        unsafe { CFRelease(self.0) };
    }
}

/// A `CFString` of `text`.
pub fn string(text: &str) -> Option<Owned> {
    // SAFETY: the bytes are valid for `len` and copied (not external).
    unsafe {
        Owned::take(CFStringCreateWithBytes(
            std::ptr::null(),
            text.as_ptr(),
            text.len() as CFIndex,
            UTF8,
            0,
        ))
    }
}

/// The text of a `CFString` someone else owns; `None` for null.
///
/// # Safety
/// `string` must be null or a valid `CFStringRef`.
pub unsafe fn to_string(string: CFStringRef) -> Option<String> {
    if string.is_null() {
        return None;
    }
    let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(string), UTF8) + 1;
    let mut buffer = vec![0u8; usize::try_from(size).ok()?];
    if CFStringGetCString(string, buffer.as_mut_ptr().cast(), size, UTF8) == 0 {
        return None;
    }
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    buffer.truncate(end);
    String::from_utf8(buffer).ok()
}

/// A `CFDictionary` of CF objects (retained by the dictionary).
pub fn dictionary(pairs: &[(CFTypeRef, CFTypeRef)]) -> Option<Owned> {
    let keys: Vec<*const c_void> = pairs.iter().map(|(k, _)| *k).collect();
    let values: Vec<*const c_void> = pairs.iter().map(|(_, v)| *v).collect();
    // SAFETY: both arrays hold `pairs.len()` valid CF objects; the CFType
    // callbacks retain them.
    unsafe {
        Owned::take(CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            pairs.len() as CFIndex,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        ))
    }
}

/// The value for `key` in a dictionary someone else owns (not retained).
///
/// # Safety
/// `dictionary` must be a valid `CFDictionaryRef`, `key` a valid CF object.
pub unsafe fn dictionary_get(dictionary: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef {
    CFDictionaryGetValue(dictionary, key)
}

/// A `CFData` with a copy of `bytes`.
pub fn data(bytes: &[u8]) -> Option<Owned> {
    // SAFETY: the bytes are valid for `len` and copied.
    unsafe {
        Owned::take(CFDataCreate(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as CFIndex,
        ))
    }
}

/// The bytes of a `CFData` someone else owns.
///
/// # Safety
/// `data` must be a valid `CFDataRef`.
pub unsafe fn data_bytes(data: CFDataRef) -> Vec<u8> {
    let len = usize::try_from(CFDataGetLength(data)).unwrap_or(0);
    let ptr = CFDataGetBytePtr(data);
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    std::slice::from_raw_parts(ptr, len).to_vec()
}

/// The items of a `CFArray` someone else owns (not retained).
///
/// # Safety
/// `array` must be a valid `CFArrayRef`.
pub unsafe fn array_items(array: CFArrayRef) -> Vec<CFTypeRef> {
    let count = CFArrayGetCount(array);
    (0..count)
        .map(|index| CFArrayGetValueAtIndex(array, index))
        .collect()
}

/// A file `CFURL` for `path`.
pub fn file_url(path: &Path, is_directory: bool) -> Option<Owned> {
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: the bytes are valid for `len`.
    unsafe {
        Owned::take(CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as CFIndex,
            u8::from(is_directory),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{string, Owned};

    #[test]
    fn taking_null_gives_nothing_and_releases_nothing() {
        // A Create or Copy function that found nothing returns null, e.g.
        // TISCreateInputSourceList for a source that isn't added. Releasing
        // it would stop the app (CFRelease(NULL); NoAX, 2026-11-24).
        // SAFETY: null is allowed and owns nothing.
        let taken = unsafe { Owned::take(std::ptr::null()) };
        assert!(taken.is_none());
    }

    #[test]
    fn an_owned_object_is_released_once_on_drop() {
        let text = string("DanyWhere").expect("a CFString");
        assert!(!text.as_ptr().is_null());
        drop(text);
    }
}
