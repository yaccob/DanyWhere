#import "DWPeerCheck.h"

#import <CommonCrypto/CommonDigest.h>
#include <errno.h>
#include <sys/socket.h>
#include <sys/un.h>

NSString *DWHexString(NSData *data) {
    const uint8_t *bytes = data.bytes;
    NSMutableString *hex = [NSMutableString stringWithCapacity:data.length * 2];
    for (NSUInteger i = 0; i < data.length; i++) [hex appendFormat:@"%02x", bytes[i]];
    return hex;
}

NSString *DWRequirementString(NSString *expectedID, NSData *leafSHA1) {
    // Bundle ids hold only letters, digits, "." and "-": nothing to escape.
    return [NSString stringWithFormat:@"identifier \"%@\" and certificate leaf = H\"%@\"", expectedID,
                                      DWHexString(leafSHA1)];
}

NSData *DWLeafCertificateSHA1OfStaticCode(SecStaticCodeRef code) {
    CFDictionaryRef info = NULL;
    if (SecCodeCopySigningInformation(code, kSecCSSigningInformation, &info) != errSecSuccess || !info) return nil;
    NSDictionary *signing = CFBridgingRelease(info);
    NSArray *certificates = signing[(__bridge NSString *)kSecCodeInfoCertificates];
    if (certificates.count == 0) return nil;
    SecCertificateRef leaf = (__bridge SecCertificateRef)certificates.firstObject;
    NSData *der = CFBridgingRelease(SecCertificateCopyData(leaf));
    if (der.length == 0) return nil;
    uint8_t digest[CC_SHA1_DIGEST_LENGTH];
    CC_SHA1(der.bytes, (CC_LONG)der.length, digest);
    return [NSData dataWithBytes:digest length:sizeof digest];
}

NSData *DWOwnLeafCertificateSHA1(void) {
    static NSData *own;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        SecCodeRef code = NULL;
        if (SecCodeCopySelf(kSecCSDefaultFlags, &code) != errSecSuccess || !code) return;
        SecStaticCodeRef staticCode = NULL;
        if (SecCodeCopyStaticCode(code, kSecCSDefaultFlags, &staticCode) == errSecSuccess && staticCode) {
            own = DWLeafCertificateSHA1OfStaticCode(staticCode);
            CFRelease(staticCode);
        }
        CFRelease(code);
    });
    return own;
}

static NSString *SecurityError(NSString *what, OSStatus status) {
    NSString *message = CFBridgingRelease(SecCopyErrorMessageString(status, NULL));
    return [NSString stringWithFormat:@"%@: %d %@", what, (int)status, message ?: @""];
}

BOOL DWCheckPeer(int fd, NSString *expectedID, NSData *ownLeafSHA1, NSString **reason) {
#define FAIL(why)                    \
    do {                             \
        if (reason) *reason = (why); \
        return NO;                   \
    } while (0)

    if (ownLeafSHA1.length == 0) FAIL(@"own code has no certificate (ad hoc or unsigned): no peer is accepted");

    audit_token_t token;
    socklen_t length = sizeof token;
    memset(&token, 0, sizeof token);
    if (getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, &token, &length) != 0) {
        FAIL(([NSString stringWithFormat:@"no peer audit token: %s", strerror(errno)]));
    }
    if (length != sizeof token) FAIL(@"peer audit token has an unexpected size");

    NSData *tokenData = [NSData dataWithBytes:&token length:sizeof token];
    NSDictionary *attributes = @{(__bridge NSString *)kSecGuestAttributeAudit: tokenData};
    SecCodeRef guest = NULL;
    OSStatus status = SecCodeCopyGuestWithAttributes(NULL, (__bridge CFDictionaryRef)attributes, kSecCSDefaultFlags, &guest);
    if (status != errSecSuccess || !guest) FAIL(SecurityError(@"peer code not found", status));

    SecRequirementRef requirement = NULL;
    NSString *text = DWRequirementString(expectedID, ownLeafSHA1);
    status = SecRequirementCreateWithString((__bridge CFStringRef)text, kSecCSDefaultFlags, &requirement);
    if (status != errSecSuccess || !requirement) {
        CFRelease(guest);
        FAIL(SecurityError(@"bad requirement", status));
    }
    status = SecCodeCheckValidity(guest, kSecCSDefaultFlags, requirement);
    CFRelease(requirement);
    CFRelease(guest);
    if (status != errSecSuccess) {
        FAIL(SecurityError([NSString stringWithFormat:@"peer (pid %d) is not %@ signed with our certificate",
                                                      (int)token.val[5], expectedID],
                           status));
    }
    return YES;
#undef FAIL
}

DWPeerCheck DWMakeSignaturePeerCheck(NSString *expectedID) {
    NSString *expected = [expectedID copy];
    return ^BOOL(int fd, NSString **reason) {
        return DWCheckPeer(fd, expected, DWOwnLeafCertificateSHA1(), reason);
    };
}
