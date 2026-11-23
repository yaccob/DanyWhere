#import "DWNames.h"

#include <unistd.h>

NSString *const DWInfoKeyAppID = @"DanyWhereAppID";
const NSUInteger DWMaxSocketPathBytes = 103;

NSString *DWInputMethodIDForAppID(NSString *appID) {
    return [appID stringByAppendingString:@".inputmethod.DanyWhere"];
}

NSString *DWConnectionNameForInputMethodID(NSString *inputMethodID) {
    return [inputMethodID stringByAppendingString:@"_Connection"];
}

NSString *DWSocketPathForAppID(NSString *appID, NSString *home, NSString *tempDir) {
    NSString *primary = [NSString stringWithFormat:@"%@/Library/Application Support/%@/input-method.sock", home, appID];
    if ([primary lengthOfBytesUsingEncoding:NSUTF8StringEncoding] <= DWMaxSocketPathBytes) {
        return primary;
    }
    // The contract concatenates as is: _CS_DARWIN_USER_TEMP_DIR ends in "/".
    return [NSString stringWithFormat:@"%@%@.input-method.sock", tempDir, appID];
}

NSString *DWUserTempDir(void) {
    char buffer[PATH_MAX];
    size_t length = confstr(_CS_DARWIN_USER_TEMP_DIR, buffer, sizeof buffer);
    if (length > 0 && length <= sizeof buffer) {
        NSString *dir = [NSString stringWithUTF8String:buffer];
        if (dir.length > 0) return dir;
    }
    NSString *fallback = NSTemporaryDirectory();
    return [fallback hasSuffix:@"/"] ? fallback : [fallback stringByAppendingString:@"/"];
}

NSString *DWLogPathForAppID(NSString *appID, NSString *home) {
    return [NSString stringWithFormat:@"%@/Library/Logs/%@/input-method.log", home, appID];
}
