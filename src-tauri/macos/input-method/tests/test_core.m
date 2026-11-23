// What the input method does with the app's messages, over a real socket to
// an in-process fake DanyWhere, with a fake text field and fake input
// sources. The test's serial delivery queue stands in for the main thread.

#import "DWTest.h"
#import "DWTestSupport.h"

#import "DWCore.h"
#import "DWLog.h"

static const NSTimeInterval kWait = 3;
static NSString *const kOwnID = @"com.example.app.inputmethod.DanyWhere";
static NSString *const kGerman = @"com.apple.keylayout.German";

@interface DWCoreRig : NSObject
@property (nonatomic, strong) DWTestServer *server;
@property (nonatomic, strong) DWCore *core;
@property (nonatomic, strong) DWFakeSystem *system;
@property (nonatomic, strong) dispatch_queue_t queue;
@property (atomic) int quits;
@end

@implementation DWCoreRig

+ (instancetype)rigWithTag:(NSString *)tag {
    DWCoreRig *rig = [[DWCoreRig alloc] init];
    NSString *path = DWTestSocketPath(tag);
    rig.server = [[DWTestServer alloc] initWithPath:path];
    rig.system = [[DWFakeSystem alloc] init];
    rig.queue = dispatch_queue_create("test.main", DISPATCH_QUEUE_SERIAL);
    DWLink *link = [[DWLink alloc] initWithSocketPath:path
                                                hello:[DWOutgoing helloWithVersion:@"1.2.3" pid:77]
                                            peerCheck:^BOOL(int fd, NSString **reason) { return YES; }];
    link.deliveryQueue = rig.queue;
    rig.core = [[DWCore alloc] initWithInputSourceID:kOwnID link:link system:rig.system];
    __weak DWCoreRig *weakRig = rig;
    rig.core.quitHandler = ^{ weakRig.quits++; };
    return rig;
}

/// Runs `block` on the stand-in main thread and waits for it.
- (void)onMain:(dispatch_block_t)block {
    dispatch_sync(self.queue, block);
}

- (BOOL)connect {
    [self.core.link connect];
    if (![self.server acceptWithin:kWait]) return NO;
    if (![[self.server readLineWithin:kWait] isEqualToString:@"{\"type\":\"hello\",\"version\":\"1.2.3\",\"pid\":77}"]) return NO;
    DWLink *link = self.core.link;
    if (!DWWaitUntil(kWait, ^BOOL { return link.connected; })) return NO;
    [self settle];
    return YES;
}

/// Lets everything sent so far reach the core.
- (void)settle {
    [self.core.link sync:^{}];
    [self onMain:^{}];
}

/// Waits until `condition`, checked on the stand-in main thread, holds.
- (BOOL)waitOnMain:(BOOL (^)(void))condition {
    return DWWaitUntil(kWait, ^BOOL {
        __block BOOL holds = NO;
        [self onMain:^{ holds = condition(); }];
        return holds;
    });
}

- (NSString *)nextLine {
    return [self.server readLineWithin:kWait];
}

- (void)tearDown {
    [self.core.link disconnect];
    [self.core.link sync:^{}];
    [self onMain:^{}];
    [self.server stop];
}

@end

static DWFakeClient *TextEdit(void) {
    return [[DWFakeClient alloc] initWithBundleID:@"com.apple.TextEdit"];
}

// --- Clients and insert ---------------------------------------------------------

DW_TEST(core_insert_without_a_client_answers_no_client) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"noclient"];
    CHECK([rig connect]);
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":7,\"text\":\"…\"}"]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inserted\",\"id\":7,\"ok\":false,\"reason\":\"no_client\"}");
    [rig tearDown];
}

DW_TEST(core_activate_sends_active_and_insert_goes_into_that_field) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"insert"];
    CHECK([rig connect]);
    DWFakeClient *client = TextEdit();
    [rig onMain:^{ [rig.core clientActivated:client]; }];
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"active\",\"client\":\"com.apple.TextEdit\"}");
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":7,\"text\":\"Hallo Welt\"}"]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inserted\",\"id\":7,\"ok\":true,\"client\":\"com.apple.TextEdit\"}");
    CHECK_EQ_OBJ(client.inserted, @[@"Hallo Welt"]);
    [rig tearDown];
}

DW_TEST(core_insert_failure_answers_its_reason) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"fail"];
    CHECK([rig connect]);
    DWFakeClient *client = TextEdit();
    client.failure = DWReasonError;
    [rig onMain:^{ [rig.core clientActivated:client]; }];
    [rig nextLine]; // active
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":8,\"text\":\"x\"}"]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inserted\",\"id\":8,\"ok\":false,\"reason\":\"error\"}");
    client.failure = DWReasonNoClient; // the controller lost its IMK client
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":9,\"text\":\"x\"}"]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inserted\",\"id\":9,\"ok\":false,\"reason\":\"no_client\"}");
    [rig tearDown];
}

DW_TEST(core_deactivate_sends_inactive_and_forgets_the_field) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"deact"];
    CHECK([rig connect]);
    DWFakeClient *client = TextEdit();
    [rig onMain:^{ [rig.core clientActivated:client]; }];
    [rig nextLine]; // active
    [rig onMain:^{ [rig.core clientDeactivated:client]; }];
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inactive\",\"client\":\"com.apple.TextEdit\"}");
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":7,\"text\":\"x\"}"]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inserted\",\"id\":7,\"ok\":false,\"reason\":\"no_client\"}");
    CHECK_EQ_INT(client.insertCalls, 0);
    [rig tearDown];
}

DW_TEST(core_late_deactivate_of_an_older_field_keeps_the_newer) {
    // IMK may activate the next field before it deactivates the previous one.
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"order"];
    CHECK([rig connect]);
    DWFakeClient *older = TextEdit();
    DWFakeClient *newer = [[DWFakeClient alloc] initWithBundleID:@"com.microsoft.teams2"];
    [rig onMain:^{
        [rig.core clientActivated:older];
        [rig.core clientActivated:newer];
        [rig.core clientDeactivated:older];
    }];
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"active\",\"client\":\"com.apple.TextEdit\"}");
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"active\",\"client\":\"com.microsoft.teams2\"}");
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inactive\",\"client\":\"com.apple.TextEdit\"}");
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":7,\"text\":\"x\"}"]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inserted\",\"id\":7,\"ok\":true,\"client\":\"com.microsoft.teams2\"}");
    CHECK_EQ_OBJ(newer.inserted, @[@"x"]);
    CHECK_EQ_INT(older.insertCalls, 0);
    [rig tearDown];
}

DW_TEST(core_closed_controller_is_forgotten_without_a_message) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"closed"];
    CHECK([rig connect]);
    DWFakeClient *client = TextEdit();
    [rig onMain:^{ [rig.core clientActivated:client]; }];
    [rig nextLine]; // active
    [rig onMain:^{ [rig.core clientClosed:client]; }];
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":7,\"text\":\"x\"}"]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inserted\",\"id\":7,\"ok\":false,\"reason\":\"no_client\"}");
    [rig tearDown];
}

DW_TEST(core_after_connecting_reports_the_active_field) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"late"];
    DWFakeClient *client = TextEdit();
    [rig onMain:^{ [rig.core clientActivated:client]; }]; // before any link: nothing to send
    // activateServer while not connected starts a connection by itself.
    CHECK([rig.server acceptWithin:kWait]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"hello\",\"version\":\"1.2.3\",\"pid\":77}");
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"active\",\"client\":\"com.apple.TextEdit\"}");
    [rig tearDown];
}

// --- Reconnecting -------------------------------------------------------------------

DW_TEST(core_tick_connects_only_while_its_source_is_selected) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"tick"];
    rig.system.current = @"com.apple.keylayout.German";
    [rig onMain:^{ [rig.core tick]; }];
    [rig.core.link sync:^{}];
    CHECK(![rig.server acceptWithin:0.3]);
    rig.system.current = kOwnID;
    [rig onMain:^{ [rig.core tick]; }];
    CHECK([rig.server acceptWithin:kWait]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"hello\",\"version\":\"1.2.3\",\"pid\":77}");
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.core.link.connected; }));
    // Connected: a tick does not connect again.
    [rig onMain:^{ [rig.core tick]; }];
    [rig.core.link sync:^{}];
    CHECK(![rig.server acceptWithin:0.3]);
    [rig tearDown];
}

// --- Layout and restore ---------------------------------------------------------------

DW_TEST(core_layout_applies_now_when_active_and_on_every_activate) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"layout"];
    CHECK([rig connect]);
    rig.system.current = kOwnID;
    // Not active: only remembered.
    CHECK([rig.server sendLine:@"{\"type\":\"layout\",\"source\":\"com.apple.keylayout.German\"}"]);
    CHECK([rig waitOnMain:^BOOL { return [rig.core.layoutSource isEqualToString:kGerman]; }]);
    CHECK_EQ_INT(rig.system.overrides.count, 0);
    // activateServer applies it.
    DWFakeClient *client = TextEdit();
    [rig onMain:^{ [rig.core clientActivated:client]; }];
    CHECK_EQ_OBJ(rig.system.overrides, @[kGerman]);
    // A new layout while active applies at once.
    CHECK([rig.server sendLine:@"{\"type\":\"layout\",\"source\":\"com.apple.keylayout.Swiss German\"}"]);
    CHECK([rig waitOnMain:^BOOL { return rig.system.overrides.count == 2; }]);
    CHECK_EQ_OBJ(rig.system.overrides, (@[kGerman, @"com.apple.keylayout.Swiss German"]));
    // null clears: back to macOS's default now, nothing on the next activate.
    CHECK([rig.server sendLine:@"{\"type\":\"layout\",\"source\":null}"]);
    CHECK([rig waitOnMain:^BOOL { return rig.system.overrides.count == 3; }]);
    CHECK_EQ_OBJ(rig.system.overrides, (@[kGerman, @"com.apple.keylayout.Swiss German", [NSNull null]]));
    __block NSString *layout = @"not cleared";
    [rig onMain:^{ layout = rig.core.layoutSource; }];
    CHECK(layout == nil);
    [rig onMain:^{
        [rig.core clientDeactivated:client];
        [rig.core clientActivated:client];
    }];
    CHECK_EQ_INT(rig.system.overrides.count, 3);
    [rig tearDown];
}

DW_TEST(core_link_loss_selects_the_restore_source_while_selected) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"restore"];
    CHECK([rig connect]);
    rig.system.current = kOwnID;
    CHECK([rig.server sendLine:@"{\"type\":\"restore\",\"source\":\"com.apple.keylayout.German\"}"]);
    [rig settle];
    [rig.server stop]; // DanyWhere crashed or quit
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.system.selected.count == 1; }));
    CHECK_EQ_OBJ(rig.system.selected, @[kGerman]);
    [rig onMain:^{}];
    __block NSString *restore = @"not cleared";
    [rig onMain:^{ restore = rig.core.restoreSource; }];
    CHECK(restore == nil); // used once
}

DW_TEST(core_link_loss_leaves_another_source_alone) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"other"];
    CHECK([rig connect]);
    rig.system.current = @"com.apple.keylayout.US"; // the user switched by hand
    CHECK([rig.server sendLine:@"{\"type\":\"restore\",\"source\":\"com.apple.keylayout.German\"}"]);
    [rig settle];
    [rig.server stop];
    CHECK(DWWaitUntil(kWait, ^BOOL { return !rig.core.link.connected; }));
    [rig onMain:^{}];
    CHECK_EQ_INT(rig.system.selected.count, 0);
}

DW_TEST(core_restore_null_clears_it) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"rnull"];
    CHECK([rig connect]);
    rig.system.current = kOwnID;
    CHECK([rig.server sendLine:@"{\"type\":\"restore\",\"source\":\"com.apple.keylayout.German\"}"]);
    CHECK([rig.server sendLine:@"{\"type\":\"restore\",\"source\":null}"]);
    [rig settle];
    [rig.server stop];
    CHECK(DWWaitUntil(kWait, ^BOOL { return !rig.core.link.connected; }));
    [rig onMain:^{}];
    CHECK_EQ_INT(rig.system.selected.count, 0);
}

// --- Stage 2 ----------------------------------------------------------------------------

static BOOL WaitArmed(DWCoreRig *rig, BOOL armed) {
    return DWWaitUntil(kWait, ^BOOL {
        __block BOOL now = NO;
        [rig onMain:^{ now = rig.core.catchState.armed; }];
        return now == armed;
    });
}

DW_TEST(core_keys_pass_through_without_a_catch) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"pass"];
    CHECK([rig connect]);
    DWFakeClient *client = TextEdit();
    [rig onMain:^{ [rig.core clientActivated:client]; }];
    [rig nextLine]; // active
    __block BOOL swallowed = YES;
    [rig onMain:^{
        swallowed = [rig.core handleKeyDownWithCode:36 modifierFlags:0 isRepeat:NO client:client] ||
                    [rig.core handleKeyDownWithCode:0 modifierFlags:0 isRepeat:NO client:client];
    }];
    CHECK(!swallowed);
    CHECK([rig.server readLineWithin:0.3] == nil); // nothing reported
    [rig tearDown];
}

DW_TEST(core_catch_release_inserts_and_lets_the_key_through) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"catch"];
    CHECK([rig connect]);
    DWFakeClient *client = TextEdit();
    [rig onMain:^{ [rig.core clientActivated:client]; }];
    [rig nextLine]; // active
    CHECK([rig.server sendLine:@"{\"type\":\"catch\",\"id\":9,\"keys\":[{\"code\":36,\"mods\":0}],\"hold_ms\":3000}"]);
    CHECK(WaitArmed(rig, YES));
    __block BOOL swallowed = YES;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    dispatch_async(rig.queue, ^{ // the main thread, held inside handleEvent:client:
        swallowed = [rig.core handleKeyDownWithCode:36 modifierFlags:0 isRepeat:NO client:client];
        dispatch_semaphore_signal(done);
    });
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"caught\",\"id\":9,\"code\":36,\"mods\":0}");
    CHECK_EQ_INT(client.insertCalls, 0);
    CHECK([rig.server sendLine:@"{\"type\":\"release\",\"id\":9,\"text\":\"Hallo\"}"]);
    CHECK(dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC)) == 0);
    CHECK(!swallowed);
    CHECK_EQ_OBJ(client.inserted, @[@"Hallo"]);
    CHECK(WaitArmed(rig, NO)); // one press per catch
    [rig tearDown];
}

DW_TEST(core_catch_without_release_swallows_the_key) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"timeout"];
    CHECK([rig connect]);
    DWFakeClient *client = TextEdit();
    [rig onMain:^{ [rig.core clientActivated:client]; }];
    [rig nextLine]; // active
    CHECK([rig.server sendLine:@"{\"type\":\"catch\",\"id\":10,\"keys\":[{\"code\":36,\"mods\":0}],\"hold_ms\":150}"]);
    CHECK(WaitArmed(rig, YES));
    __block BOOL swallowed = NO;
    NSDate *start = [NSDate date];
    [rig onMain:^{ swallowed = [rig.core handleKeyDownWithCode:36 modifierFlags:0 isRepeat:NO client:client]; }];
    double ms = [[NSDate date] timeIntervalSinceDate:start] * 1000;
    CHECK(swallowed);
    CHECK(ms >= 120);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"caught\",\"id\":10,\"code\":36,\"mods\":0}");
    // The text comes later with insert.
    CHECK([rig.server sendLine:@"{\"type\":\"release\",\"id\":10,\"text\":\"zu spät\"}"]);
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":11,\"text\":\"zu spät\"}"]);
    CHECK_EQ_OBJ([rig nextLine], @"{\"type\":\"inserted\",\"id\":11,\"ok\":true,\"client\":\"com.apple.TextEdit\"}");
    CHECK_EQ_OBJ(client.inserted, @[@"zu spät"]); // once, from insert
    [rig tearDown];
}

DW_TEST(core_catch_disarm_and_link_loss) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"disarm"];
    CHECK([rig connect]);
    CHECK([rig.server sendLine:@"{\"type\":\"catch\",\"id\":12,\"keys\":[{\"code\":36,\"mods\":0}],\"hold_ms\":500}"]);
    CHECK(WaitArmed(rig, YES));
    CHECK([rig.server sendLine:@"{\"type\":\"catch\",\"id\":12,\"keys\":[]}"]);
    CHECK(WaitArmed(rig, NO));
    CHECK([rig.server sendLine:@"{\"type\":\"catch\",\"id\":13,\"keys\":[{\"code\":36,\"mods\":0}],\"hold_ms\":500}"]);
    CHECK(WaitArmed(rig, YES));
    [rig.server stop]; // whoever armed it is gone
    CHECK(WaitArmed(rig, NO));
}

DW_TEST(core_quit_calls_the_quit_handler) {
    DWCoreRig *rig = [DWCoreRig rigWithTag:@"quit"];
    CHECK([rig connect]);
    CHECK([rig.server sendLine:@"{\"type\":\"quit\"}"]);
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.quits == 1; }));
    [rig tearDown];
}

// --- The log ---------------------------------------------------------------------------------

DW_TEST(core_log_has_events_but_never_text) {
    NSString *dir = [NSTemporaryDirectory() stringByAppendingPathComponent:
                                                [NSString stringWithFormat:@"dwim-log-%d", getpid()]];
    NSString *path = [dir stringByAppendingPathComponent:@"input-method.log"];
    [[NSFileManager defaultManager] removeItemAtPath:dir error:NULL];
    [[DWLog shared] configureWithPath:path maxBytes:1 << 20];

    DWCoreRig *rig = [DWCoreRig rigWithTag:@"log"];
    CHECK([rig connect]);
    DWFakeClient *client = TextEdit();
    [rig onMain:^{ [rig.core clientActivated:client]; }];
    [rig nextLine]; // active
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":1,\"text\":\"GEHEIM-eins\"}"]);
    [rig nextLine]; // inserted
    CHECK([rig.server sendLine:@"{\"type\":\"catch\",\"id\":2,\"keys\":[{\"code\":36,\"mods\":0}],\"hold_ms\":3000}"]);
    CHECK(WaitArmed(rig, YES));
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    dispatch_async(rig.queue, ^{
        [rig.core handleKeyDownWithCode:36 modifierFlags:0 isRepeat:NO client:client];
        dispatch_semaphore_signal(done);
    });
    [rig nextLine]; // caught
    CHECK([rig.server sendLine:@"{\"type\":\"release\",\"id\":2,\"text\":\"GEHEIM-zwei\"}"]);
    CHECK(dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC)) == 0);
    [rig.core.link disconnect];
    [rig settle];
    [rig onMain:^{ [rig.core handleKeyDownWithCode:0 modifierFlags:0 isRepeat:NO client:client]; }];
    [[DWLog shared] flush];
    [[DWLog shared] configureWithPath:nil maxBytes:0];

    NSString *log = [NSString stringWithContentsOfFile:path encoding:NSUTF8StringEncoding error:NULL];
    CHECK(log.length > 0);
    CHECK([log rangeOfString:@"GEHEIM"].location == NSNotFound);
    CHECK([log rangeOfString:@"com.apple.TextEdit"].location != NSNotFound); // which app, not what
    NSArray<NSString *> *events = @[@"connected", @"activate", @"insert", @"catch"];
    for (NSString *event in events) {
        BOOL found = [log rangeOfString:event options:NSCaseInsensitiveSearch].location != NSNotFound;
        DWTestCheck(found, "log names the event", __FILE__, __LINE__, found ? nil : event);
    }
    [rig.server stop];
    [[NSFileManager defaultManager] removeItemAtPath:dir error:NULL];
}

DW_TEST(log_rotates_at_its_size_cap) {
    NSString *dir = [NSTemporaryDirectory() stringByAppendingPathComponent:
                                                [NSString stringWithFormat:@"dwim-rotate-%d/deeper", getpid()]];
    NSString *path = [dir stringByAppendingPathComponent:@"input-method.log"];
    [[NSFileManager defaultManager] removeItemAtPath:[dir stringByDeletingLastPathComponent] error:NULL];
    [[DWLog shared] configureWithPath:path maxBytes:400];
    for (int i = 0; i < 40; i++) DWLogEvent(@"event number %d with some words", i);
    [[DWLog shared] flush];
    [[DWLog shared] configureWithPath:nil maxBytes:0];
    NSFileManager *files = [NSFileManager defaultManager];
    unsigned long long size = [[files attributesOfItemAtPath:path error:NULL] fileSize];
    unsigned long long oldSize = [[files attributesOfItemAtPath:[path stringByAppendingString:@".1"] error:NULL] fileSize];
    CHECK(size > 0);
    CHECK(size <= 400);
    CHECK(oldSize > 0);
    CHECK(oldSize <= 400);
    NSString *log = [NSString stringWithContentsOfFile:path encoding:NSUTF8StringEncoding error:NULL];
    CHECK([log rangeOfString:@"event number 39 "].location != NSNotFound); // the newest is kept
    [files removeItemAtPath:[dir stringByDeletingLastPathComponent] error:NULL];
}
