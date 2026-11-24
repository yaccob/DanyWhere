// Names and paths of the input method, derived from the app's bundle id
// (see ../PROTOCOL.md, "Names" and "The link").

#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// Custom Info.plist key: the bundle id of the app this input method belongs to.
extern NSString *const DWInfoKeyAppID; // "DanyWhereAppID"

/// The longest socket path `sockaddr_un.sun_path` takes, in bytes, without
/// its terminating NUL.
extern const NSUInteger DWMaxSocketPathBytes; // 103

/// `<app id>.inputmethod.DanyWhere` (CFBundleIdentifier = TISInputSourceID).
NSString *DWInputMethodIDForAppID(NSString *appID);

/// `<input method id>_Connection` (InputMethodConnectionName).
NSString *DWConnectionNameForInputMethodID(NSString *inputMethodID);

/// The socket path: `<home>/Library/Application Support/<app id>/input-method.sock`,
/// or `<tempDir><app id>.input-method.sock` when the first is longer than
/// DWMaxSocketPathBytes. `tempDir` is `_CS_DARWIN_USER_TEMP_DIR` (ends in "/").
NSString *DWSocketPathForAppID(NSString *appID, NSString *home, NSString *tempDir);

/// `_CS_DARWIN_USER_TEMP_DIR` of this user, or NSTemporaryDirectory().
NSString *DWUserTempDir(void);

/// `<home>/Library/Logs/<app id>/input-method.log`.
NSString *DWLogPathForAppID(NSString *appID, NSString *home);

NS_ASSUME_NONNULL_END
