// The socket link against an in-process fake DanyWhere, with the peer check
// replaced (the real one is tested in test_peer.m).

#import "DWTest.h"
#import "DWTestSupport.h"

#import "DWLink.h"

static const NSTimeInterval kWait = 3;
static void *kDeliveryQueueKey = &kDeliveryQueueKey;

@interface DWLinkRecorder : NSObject <DWLinkDelegate>
@property (atomic) int connects;
@property (atomic) int disconnects;
@property (atomic, copy) NSString *lastDisconnectReason;
@property (atomic, readonly) NSArray<DWIncoming *> *messages;
@property (atomic, readonly) NSArray<DWIncoming *> *releases;
@property (atomic) int releasesOnDeliveryQueue;
@end

@implementation DWLinkRecorder {
    NSMutableArray<DWIncoming *> *_messages;
    NSMutableArray<DWIncoming *> *_releases;
}

- (instancetype)init {
    if ((self = [super init])) {
        _messages = [NSMutableArray array];
        _releases = [NSMutableArray array];
    }
    return self;
}

- (void)linkDidConnect:(DWLink *)link {
    self.connects++;
}

- (void)linkDidDisconnect:(DWLink *)link reason:(NSString *)reason {
    self.lastDisconnectReason = reason;
    self.disconnects++;
}

- (void)link:(DWLink *)link didReceiveMessage:(DWIncoming *)message {
    @synchronized(self) {
        [_messages addObject:message];
    }
}

- (void)link:(DWLink *)link didReceiveRelease:(DWIncoming *)message {
    if (dispatch_get_specific(kDeliveryQueueKey)) self.releasesOnDeliveryQueue++;
    @synchronized(self) {
        [_releases addObject:message];
    }
}

- (NSArray<DWIncoming *> *)messages {
    @synchronized(self) {
        return [_messages copy];
    }
}

- (NSArray<DWIncoming *> *)releases {
    @synchronized(self) {
        return [_releases copy];
    }
}

@end

@interface DWLinkRig : NSObject
@property (nonatomic, strong) DWTestServer *server;
@property (nonatomic, strong) DWLink *link;
@property (nonatomic, strong) DWLinkRecorder *recorder;
@property (nonatomic, strong) dispatch_queue_t deliveryQueue;
@property (atomic) int peerChecks;
@property (atomic) BOOL acceptPeer;
@end

@implementation DWLinkRig

+ (instancetype)rigWithTag:(NSString *)tag {
    DWLinkRig *rig = [[DWLinkRig alloc] init];
    NSString *path = DWTestSocketPath(tag);
    rig.server = [[DWTestServer alloc] initWithPath:path];
    rig.acceptPeer = YES;
    __weak DWLinkRig *weakRig = rig;
    rig.link = [[DWLink alloc] initWithSocketPath:path
                                            hello:[DWOutgoing helloWithVersion:@"9.9.9" pid:4242]
                                        peerCheck:^BOOL(int fd, NSString **reason) {
                                            DWLinkRig *strong = weakRig;
                                            strong.peerChecks++;
                                            if (!strong.acceptPeer) *reason = @"refused by the test";
                                            return strong.acceptPeer;
                                        }];
    rig.recorder = [[DWLinkRecorder alloc] init];
    rig.deliveryQueue = dispatch_queue_create("test.delivery", DISPATCH_QUEUE_SERIAL);
    dispatch_queue_set_specific(rig.deliveryQueue, kDeliveryQueueKey, kDeliveryQueueKey, NULL);
    rig.link.deliveryQueue = rig.deliveryQueue;
    rig.link.delegate = rig.recorder;
    return rig;
}

- (BOOL)connectAndReadHello {
    int before = self.recorder.connects;
    [self.link connect];
    if (![self.server acceptWithin:kWait]) return NO;
    NSString *hello = [self.server readLineWithin:kWait];
    if (![hello isEqualToString:@"{\"type\":\"hello\",\"version\":\"9.9.9\",\"pid\":4242}"]) return NO;
    DWLinkRecorder *recorder = self.recorder;
    DWLink *link = self.link;
    return DWWaitUntil(kWait, ^BOOL {
        return recorder.connects > before && link.connected;
    });
}

- (void)tearDown {
    [self.link disconnect];
    [self.link sync:^{}];
    [self.server stop];
}

@end

DW_TEST(link_sends_hello_first_after_the_peer_check) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"hello"];
    CHECK(rig.server != nil);
    CHECK(!rig.link.connected);
    [rig.link connect];
    CHECK([rig.server acceptWithin:kWait]);
    CHECK_EQ_OBJ([rig.server readLineWithin:kWait], @"{\"type\":\"hello\",\"version\":\"9.9.9\",\"pid\":4242}");
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.recorder.connects == 1; }));
    CHECK(rig.link.connected);
    CHECK_EQ_INT(rig.peerChecks, 1);
    [rig tearDown];
}

DW_TEST(link_refused_peer_gets_nothing_and_is_closed) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"refuse"];
    rig.acceptPeer = NO;
    [rig.link connect];
    CHECK([rig.server acceptWithin:kWait]);
    CHECK([rig.server clientClosedWithin:kWait]); // EOF before any byte
    [rig.link sync:^{}];
    CHECK_EQ_INT(rig.peerChecks, 1);
    CHECK(!rig.link.connected);
    CHECK_EQ_INT(rig.recorder.connects, 0);
    CHECK_EQ_INT(rig.recorder.disconnects, 0); // it never was up
    [rig tearDown];
}

DW_TEST(link_connect_without_a_listener_stays_disconnected) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"nolisten"];
    [rig.server stop];
    [rig.link connect];
    [rig.link sync:^{}];
    CHECK(!rig.link.connected);
    CHECK_EQ_INT(rig.peerChecks, 0);
    CHECK_EQ_INT(rig.recorder.connects, 0);
}

DW_TEST(link_connect_while_connected_does_nothing) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"twice"];
    CHECK([rig connectAndReadHello]);
    [rig.link connect];
    [rig.link sync:^{}];
    CHECK(![rig.server acceptWithin:0.3]);
    CHECK_EQ_INT(rig.recorder.connects, 1);
    CHECK_EQ_INT(rig.peerChecks, 1);
    [rig tearDown];
}

DW_TEST(link_sends_lines_and_drops_them_while_disconnected) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"send"];
    [rig.link send:[DWOutgoing activeWithClient:@"before.connect"]]; // dropped
    CHECK([rig connectAndReadHello]);
    [rig.link send:[DWOutgoing activeWithClient:@"com.apple.TextEdit"]];
    [rig.link send:[DWOutgoing inactiveWithClient:@"com.apple.TextEdit"]];
    CHECK_EQ_OBJ([rig.server readLineWithin:kWait], @"{\"type\":\"active\",\"client\":\"com.apple.TextEdit\"}");
    CHECK_EQ_OBJ([rig.server readLineWithin:kWait], @"{\"type\":\"inactive\",\"client\":\"com.apple.TextEdit\"}");
    [rig tearDown];
}

DW_TEST(link_delivers_known_messages_and_skips_the_rest) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"deliver"];
    CHECK([rig connectAndReadHello]);
    CHECK([rig.server sendLine:@"{\"type\":\"insert\",\"id\":7,\"text\":\"…\"}"]);
    CHECK([rig.server sendLine:@"this is not json"]);
    CHECK([rig.server sendLine:@"{\"type\":\"from_the_future\",\"id\":1}"]);
    CHECK([rig.server sendLine:@"{\"type\":\"layout\",\"source\":\"com.apple.keylayout.German\",\"extra\":true}"]);
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.recorder.messages.count >= 2; }));
    [rig.link sync:^{}];
    dispatch_sync(rig.deliveryQueue, ^{});
    NSArray<DWIncoming *> *messages = rig.recorder.messages;
    CHECK_EQ_INT(messages.count, 2);
    CHECK_EQ_INT(messages.firstObject.kind, DWMessageInsert);
    CHECK_EQ_OBJ(messages.firstObject.text, @"…");
    CHECK_EQ_INT(messages.lastObject.kind, DWMessageLayout);
    CHECK(rig.link.connected); // a bad line does not break the link
    [rig tearDown];
}

DW_TEST(link_release_arrives_while_the_delivery_queue_is_blocked) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"release"];
    CHECK([rig connectAndReadHello]);
    // Stands in for the main thread held inside handleEvent:client:.
    dispatch_semaphore_t blocked = dispatch_semaphore_create(0);
    dispatch_semaphore_t unblock = dispatch_semaphore_create(0);
    dispatch_async(rig.deliveryQueue, ^{
        dispatch_semaphore_signal(blocked);
        dispatch_semaphore_wait(unblock, dispatch_time(DISPATCH_TIME_NOW, 10 * NSEC_PER_SEC));
    });
    dispatch_semaphore_wait(blocked, DISPATCH_TIME_FOREVER);
    CHECK([rig.server sendLine:@"{\"type\":\"release\",\"id\":9,\"text\":\"…\"}"]);
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.recorder.releases.count == 1; }));
    CHECK_EQ_INT(rig.recorder.releases.firstObject.messageID, 9);
    CHECK_EQ_OBJ(rig.recorder.releases.firstObject.text, @"…");
    CHECK_EQ_INT(rig.recorder.releasesOnDeliveryQueue, 0);
    dispatch_semaphore_signal(unblock);
    [rig tearDown];
}

DW_TEST(link_reconnects_after_the_server_goes_away) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"reconnect"];
    NSString *path = rig.server.path;
    CHECK([rig connectAndReadHello]);
    [rig.server stop]; // DanyWhere quits: connection and socket file gone
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.recorder.disconnects == 1; }));
    CHECK(!rig.link.connected);
    CHECK(rig.recorder.lastDisconnectReason.length > 0);
    [rig.link connect]; // nobody listens yet
    [rig.link sync:^{}];
    CHECK(!rig.link.connected);
    rig.server = [[DWTestServer alloc] initWithPath:path]; // DanyWhere starts again
    CHECK(rig.server != nil);
    CHECK([rig connectAndReadHello]);
    CHECK_EQ_INT(rig.recorder.connects, 2);
    CHECK_EQ_INT(rig.peerChecks, 2);
    // The new connection works.
    CHECK([rig.server sendLine:@"{\"type\":\"quit\"}"]);
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.recorder.messages.count == 1; }));
    CHECK_EQ_INT(rig.recorder.messages.firstObject.kind, DWMessageQuit);
    [rig tearDown];
}

DW_TEST(link_partial_line_of_a_dead_connection_is_not_carried_over) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"partial"];
    NSString *path = rig.server.path;
    CHECK([rig connectAndReadHello]);
    // Half a line, then the connection dies.
    CHECK([rig.server sendRaw:@"{\"type\":\"insert\",\"id\":1,"]);
    [rig.server stop];
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.recorder.disconnects == 1; }));
    rig.server = [[DWTestServer alloc] initWithPath:path];
    CHECK([rig connectAndReadHello]);
    CHECK([rig.server sendLine:@"{\"type\":\"quit\"}"]);
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.recorder.messages.count == 1; }));
    CHECK_EQ_INT(rig.recorder.messages.firstObject.kind, DWMessageQuit);
    [rig tearDown];
}

DW_TEST(link_disconnect_closes_the_socket) {
    DWLinkRig *rig = [DWLinkRig rigWithTag:@"close"];
    CHECK([rig connectAndReadHello]);
    [rig.link disconnect];
    CHECK([rig.server clientClosedWithin:kWait]);
    CHECK(DWWaitUntil(kWait, ^BOOL { return rig.recorder.disconnects == 1; }));
    CHECK(!rig.link.connected);
    [rig.server stop];
}
