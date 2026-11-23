#import "DWSystem.h"

#import <Carbon/Carbon.h>

/// The input source with that id: enabled ones first, then every installed
/// one (a keyboard layout override need not be enabled).
static TISInputSourceRef CopySource(NSString *sourceID) {
    NSDictionary *properties = @{(__bridge NSString *)kTISPropertyInputSourceID: sourceID};
    for (int includeAll = 0; includeAll <= 1; includeAll++) {
        CFArrayRef list = TISCreateInputSourceList((__bridge CFDictionaryRef)properties, includeAll ? true : false);
        if (!list) continue;
        TISInputSourceRef source = NULL;
        if (CFArrayGetCount(list) > 0) {
            source = (TISInputSourceRef)CFRetain(CFArrayGetValueAtIndex(list, 0));
        }
        CFRelease(list);
        if (source) return source;
    }
    return NULL;
}

@implementation DWSystemTIS

- (NSString *)currentInputSourceID {
    TISInputSourceRef current = TISCopyCurrentKeyboardInputSource();
    if (!current) return nil;
    NSString *sourceID = (__bridge NSString *)TISGetInputSourceProperty(current, kTISPropertyInputSourceID);
    NSString *copy = [sourceID copy];
    CFRelease(current);
    return copy;
}

- (BOOL)selectInputSourceWithID:(NSString *)sourceID reason:(NSString **)reason {
    TISInputSourceRef source = CopySource(sourceID);
    if (!source) {
        if (reason) *reason = @"no such input source";
        return NO;
    }
    OSStatus status = TISSelectInputSource(source);
    CFRelease(source);
    if (status != noErr) {
        if (reason) *reason = [NSString stringWithFormat:@"TISSelectInputSource: %d", (int)status];
        return NO;
    }
    return YES;
}

- (BOOL)setKeyboardLayoutOverrideWithID:(NSString *)sourceID reason:(NSString **)reason {
    TISInputSourceRef layout = sourceID ? CopySource(sourceID) : TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
    if (!layout) {
        if (reason) *reason = @"no such keyboard layout";
        return NO;
    }
    OSStatus status = TISSetInputMethodKeyboardLayoutOverride(layout);
    CFRelease(layout);
    if (status != noErr) {
        if (reason) *reason = [NSString stringWithFormat:@"TISSetInputMethodKeyboardLayoutOverride: %d", (int)status];
        return NO;
    }
    return YES;
}

@end
