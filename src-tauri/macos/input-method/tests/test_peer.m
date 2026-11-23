// The peer check: requirement string, own certificate, and the real check on
// a socket connected to this very process.
//
// build.sh --test signs the test runner with "DanyWhere Local Signing" when
// that identity is in the keychain and passes its identifier in
// DW_TEST_SIGNED_ID; without it the real-signature test is skipped.

#import "DWTest.h"
#import "DWTestSupport.h"

#import "DWPeerCheck.h"

DW_TEST(peer_hex_is_lowercase) {
    const uint8_t bytes[] = {0x00, 0x0f, 0xa0, 0xff, 0x12};
    CHECK_EQ_OBJ(DWHexString([NSData dataWithBytes:bytes length:sizeof bytes]), @"000fa0ff12");
    CHECK_EQ_OBJ(DWHexString([NSData data]), @"");
}

DW_TEST(peer_requirement_string_names_identifier_and_leaf) {
    uint8_t sha1[20];
    for (int i = 0; i < 20; i++) sha1[i] = (uint8_t)(0x10 + i);
    NSString *requirement = DWRequirementString(@"com.pais.handy.dev.inputmethod.DanyWhere",
                                                [NSData dataWithBytes:sha1 length:sizeof sha1]);
    CHECK_EQ_OBJ(requirement,
                 @"identifier \"com.pais.handy.dev.inputmethod.DanyWhere\" and certificate leaf = H\"101112131415161718191a1b1c1d1e1f20212223\"");
    // macOS accepts it as a code requirement.
    SecRequirementRef parsed = NULL;
    OSStatus status = SecRequirementCreateWithString((__bridge CFStringRef)requirement, kSecCSDefaultFlags, &parsed);
    CHECK_EQ_INT(status, errSecSuccess);
    if (parsed) CFRelease(parsed);
}

DW_TEST(peer_leaf_certificate_of_a_signed_system_binary) {
    SecStaticCodeRef code = NULL;
    NSURL *url = [NSURL fileURLWithPath:@"/usr/bin/true"];
    OSStatus status = SecStaticCodeCreateWithPath((__bridge CFURLRef)url, kSecCSDefaultFlags, &code);
    CHECK_EQ_INT(status, errSecSuccess);
    if (!code) return;
    NSData *sha1 = DWLeafCertificateSHA1OfStaticCode(code);
    CFRelease(code);
    CHECK(sha1 != nil);
    CHECK_EQ_INT(sha1.length, 20);
}

DW_TEST(peer_without_own_certificate_accepts_no_peer) {
    DWSocketPair *pair = [DWSocketPair connectedPair];
    CHECK(pair != nil);
    NSString *reason = nil;
    CHECK(!DWCheckPeer(pair.serverFD, @"anything", nil, &reason));
    CHECK(reason.length > 0);
    [pair close];
}

DW_TEST(peer_check_fails_on_a_non_socket) {
    int fds[2];
    CHECK_EQ_INT(pipe(fds), 0);
    uint8_t fake[20] = {0};
    NSString *reason = nil;
    CHECK(!DWCheckPeer(fds[0], @"anything", [NSData dataWithBytes:fake length:20], &reason));
    CHECK(reason.length > 0);
    close(fds[0]);
    close(fds[1]);
}

DW_TEST(peer_check_with_a_real_signature) {
    NSString *signedID = NSProcessInfo.processInfo.environment[@"DW_TEST_SIGNED_ID"];
    NSData *own = DWOwnLeafCertificateSHA1();
    if (signedID.length == 0) {
        // Ad hoc (linker-signed) runner: it has no certificate, so the
        // check built for it rejects every peer.
        CHECK(own == nil);
        DWSocketPair *pair = [DWSocketPair connectedPair];
        NSString *reason = nil;
        CHECK(!DWMakeSignaturePeerCheck(@"anything")(pair.serverFD, &reason));
        [pair close];
        DWTestSkip(@"test runner not signed with a certificate (no \"DanyWhere Local Signing\" identity)");
        return;
    }
    CHECK(own != nil);
    CHECK_EQ_INT(own.length, 20);
    DWSocketPair *pair = [DWSocketPair connectedPair];
    CHECK(pair != nil);
    NSString *reason = nil;
    // Both ends are this process: same identifier, same certificate.
    BOOL serverSide = DWCheckPeer(pair.serverFD, signedID, own, &reason);
    CHECK(serverSide);
    if (!serverSide) NSLog(@"server side: %@", reason);
    reason = nil;
    BOOL clientSide = DWMakeSignaturePeerCheck(signedID)(pair.clientFD, &reason);
    CHECK(clientSide);
    if (!clientSide) NSLog(@"client side: %@", reason);
    // Another identifier is refused.
    reason = nil;
    CHECK(!DWCheckPeer(pair.serverFD, @"com.example.someone.else", own, &reason));
    CHECK(reason.length > 0);
    // Another certificate is refused.
    uint8_t other[20];
    memset(other, 0xab, sizeof other);
    reason = nil;
    CHECK(!DWCheckPeer(pair.serverFD, signedID, [NSData dataWithBytes:other length:20], &reason));
    CHECK(reason.length > 0);
    [pair close];
}
