#import "DWCore.h"

#import "DWLog.h"

/// While its source is selected and not connected, the input method tries
/// to connect this often (PROTOCOL.md, "The link").
static const NSTimeInterval kReconnectInterval = 2.0;

static DWCore *gShared;

static NSString *Shown(NSString *bundleID) {
    return bundleID.length > 0 ? bundleID : @"?";
}

static double MillisecondsSince(CFAbsoluteTime start) {
    return (CFAbsoluteTimeGetCurrent() - start) * 1000.0;
}

@implementation DWCore {
    id<DWSystem> _system;
    NSTimer *_timer;
    /// An `insert` that came while no text field had the input method; it
    /// goes into the next activated field, or answers `no_client` after
    /// `clientWait`.
    DWIncoming *_pendingInsert;
    uint64_t _pendingGeneration;
}

+ (DWCore *)shared {
    @synchronized(self) {
        return gShared;
    }
}

+ (void)setShared:(DWCore *)shared {
    @synchronized(self) {
        gShared = shared;
    }
}

- (instancetype)initWithInputSourceID:(NSString *)inputSourceID link:(DWLink *)link system:(id<DWSystem>)system {
    if ((self = [super init])) {
        _inputSourceID = [inputSourceID copy];
        _link = link;
        _system = system;
        _catchState = [[DWCatchState alloc] init];
        _holdGate = [[DWHoldGate alloc] init];
        _clientWait = 0.5;
        _quitHandler = ^{
            [[DWLog shared] flush];
            exit(0);
        };
        link.delegate = self;
    }
    return self;
}

- (void)start {
    [_link connect];
    _timer = [NSTimer timerWithTimeInterval:kReconnectInterval
                                     target:self
                                   selector:@selector(timerFired:)
                                   userInfo:nil
                                    repeats:YES];
    _timer.tolerance = 0.5;
    [[NSRunLoop mainRunLoop] addTimer:_timer forMode:NSRunLoopCommonModes];
}

- (void)timerFired:(NSTimer *)timer {
    [self tick];
}

- (void)tick {
    if (_link.connected) return;
    if ([[_system currentInputSourceID] isEqualToString:_inputSourceID]) [_link connect];
}

#pragma mark - Text fields

- (void)clientActivated:(id<DWTextClient>)client {
    _currentClient = client;
    NSString *bundleID = client.clientBundleID ?: @"";
    DWLogEvent(@"activate: %@", Shown(bundleID));
    [_link send:[DWOutgoing activeWithClient:bundleID]];
    if (_layoutSource) [self applyLayout];
    if (!_link.connected) [_link connect];
    DWIncoming *pending = _pendingInsert;
    if (pending) {
        _pendingInsert = nil;
        [self handleInsert:pending];
    }
}

- (void)clientDeactivated:(id<DWTextClient>)client {
    NSString *bundleID = client.clientBundleID ?: @"";
    DWLogEvent(@"deactivate: %@", Shown(bundleID));
    [_link send:[DWOutgoing inactiveWithClient:bundleID]];
    if (_currentClient == client) _currentClient = nil;
}

- (void)clientClosed:(id<DWTextClient>)client {
    if (_currentClient == client) _currentClient = nil;
}

- (void)applyLayout {
    NSString *reason = nil;
    BOOL ok = [_system setKeyboardLayoutOverrideWithID:_layoutSource reason:&reason];
    if (ok) {
        DWLogEvent(@"layout: override %@ applied", _layoutSource ?: @"cleared (macOS default)");
    } else {
        DWLogEvent(@"layout: override %@ failed: %@", _layoutSource ?: @"(default)", reason ?: @"?");
    }
}

#pragma mark - Messages

/// Keeps `message` until a text field gets the input method, at most
/// `clientWait`; a newer insert replaces an older one, which answers
/// `no_client` at once.
- (void)waitForClientWithInsert:(DWIncoming *)message {
    DWIncoming *older = _pendingInsert;
    if (older) {
        DWLogEvent(@"insert %lld failed: %@ (a newer insert came)", older.messageID, DWReasonNoClient);
        [_link send:[DWOutgoing insertedFailedWithID:older.messageID reason:DWReasonNoClient]];
    }
    _pendingInsert = message;
    uint64_t generation = ++_pendingGeneration;
    DWLogEvent(@"insert %lld: no text field yet, waiting up to %.0f ms", message.messageID, _clientWait * 1000.0);
    dispatch_queue_t queue = _link.deliveryQueue ?: dispatch_get_main_queue();
    __weak DWCore *weakSelf = self;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(_clientWait * NSEC_PER_SEC)), queue, ^{
        [weakSelf pendingInsertTimedOut:generation];
    });
}

- (void)pendingInsertTimedOut:(uint64_t)generation {
    DWIncoming *pending = _pendingInsert;
    if (!pending || generation != _pendingGeneration) return;
    _pendingInsert = nil;
    DWLogEvent(@"insert %lld failed: %@", pending.messageID, DWReasonNoClient);
    [_link send:[DWOutgoing insertedFailedWithID:pending.messageID reason:DWReasonNoClient]];
}

- (void)handleInsert:(DWIncoming *)message {
    id<DWTextClient> client = _currentClient;
    if (!client) {
        [self waitForClientWithInsert:message];
        return;
    }
    NSString *bundleID = client.clientBundleID;
    CFAbsoluteTime start = CFAbsoluteTimeGetCurrent();
    NSString *failure = [client insertText:message.text];
    double ms = MillisecondsSince(start);
    if (failure) {
        DWLogEvent(@"insert %lld into %@ failed: %@ (%.0f ms)", message.messageID, Shown(bundleID), failure, ms);
        [_link send:[DWOutgoing insertedFailedWithID:message.messageID reason:failure]];
        return;
    }
    DWLogEvent(@"insert %lld into %@ ok (%.0f ms)", message.messageID, Shown(bundleID), ms);
    [_link send:[DWOutgoing insertedOKWithID:message.messageID client:bundleID]];
}

- (void)handleCatch:(DWIncoming *)message {
    if (message.keys.count == 0) {
        if (_catchState.armed) DWLogEvent(@"catch %lld: disarmed", message.messageID);
        [_catchState disarm];
        return;
    }
    [_catchState armWithID:message.messageID keys:message.keys holdMs:message.holdMs];
    DWLogEvent(@"catch %lld: armed for %lu key(s), holding up to %ld ms, field: %@", message.messageID,
               (unsigned long)message.keys.count, (long)message.holdMs,
               _currentClient ? Shown(_currentClient.clientBundleID) : @"none");
}

- (void)link:(DWLink *)link didReceiveMessage:(DWIncoming *)message {
    switch (message.kind) {
        case DWMessageInsert:
            [self handleInsert:message];
            break;
        case DWMessageLayout:
            _layoutSource = [message.source copy];
            DWLogEvent(@"layout: %@", _layoutSource ?: @"cleared");
            if (_currentClient) [self applyLayout];
            break;
        case DWMessageRestore:
            _restoreSource = [message.source copy];
            DWLogEvent(@"restore: %@", _restoreSource ?: @"cleared");
            break;
        case DWMessageCatch:
            [self handleCatch:message];
            break;
        case DWMessageRelease:
            [self link:link didReceiveRelease:message];
            break;
        case DWMessageQuit:
            DWLogEvent(@"quit: requested by DanyWhere");
            if (self.quitHandler) self.quitHandler();
            break;
        case DWMessageUnknown:
            break;
    }
}

- (void)link:(DWLink *)link didReceiveRelease:(DWIncoming *)message {
    // On the link's queue: the main thread may be waiting in handleKeyDown.
    if (![_holdGate releaseID:message.messageID text:message.text]) {
        DWLogEvent(@"catch %lld: release dropped, no key is held for it (too late or unknown)", message.messageID);
    }
}

- (void)linkDidConnect:(DWLink *)link {
    // DanyWhere (re)started while a text field has the input method.
    if (_currentClient) [_link send:[DWOutgoing activeWithClient:_currentClient.clientBundleID ?: @""]];
}

- (void)linkDidDisconnect:(DWLink *)link reason:(NSString *)reason {
    if (_pendingInsert) {
        // The app no longer waits for it and may insert the text another way.
        DWLogEvent(@"insert %lld: dropped, the link is gone", _pendingInsert.messageID);
        _pendingInsert = nil;
    }
    if (_catchState.armed) {
        DWLogEvent(@"catch %lld: disarmed, the link is gone", _catchState.armedID);
        [_catchState disarm];
    }
    NSString *restore = _restoreSource;
    _restoreSource = nil; // it belongs to the connection that sent it
    if (!restore) return;
    NSString *current = [_system currentInputSourceID];
    if (![current isEqualToString:_inputSourceID]) {
        DWLogEvent(@"restore: link lost while %@ is selected, nothing to restore", current ?: @"?");
        return;
    }
    NSString *why = nil;
    if ([_system selectInputSourceWithID:restore reason:&why]) {
        DWLogEvent(@"restore: link lost while selected, %@ selected again", restore);
    } else {
        DWLogEvent(@"restore: selecting %@ failed: %@", restore, why ?: @"?");
    }
}

#pragma mark - Keys

- (BOOL)handleKeyDownWithCode:(uint16_t)code
                modifierFlags:(NSUInteger)flags
                     isRepeat:(BOOL)isRepeat
                       client:(id<DWTextClient>)client {
    if (!_catchState.armed) return NO;
    long long catchID = _catchState.armedID;
    NSInteger holdMs = _catchState.holdMs;
    if (![_catchState takeKeyCode:code modifierFlags:flags isRepeat:isRepeat hasClient:client != nil]) return NO;

    NSString *bundleID = Shown(client.clientBundleID);
    [_holdGate beginForID:catchID];
    [_link send:[DWOutgoing caughtWithID:catchID code:code mods:DWChordModifiers(flags)]];
    DWLogEvent(@"catch %lld: caught in %@, holding the key up to %ld ms", catchID, bundleID, (long)holdMs);

    CFAbsoluteTime start = CFAbsoluteTimeGetCurrent();
    NSString *text = nil;
    DWHoldOutcome outcome = [_holdGate waitMilliseconds:holdMs text:&text];
    double waited = MillisecondsSince(start);
    if (outcome == DWHoldTimedOut) {
        DWLogEvent(@"catch %lld: timed out after %.0f ms, key swallowed (the text comes with insert)", catchID, waited);
        return YES;
    }
    CFAbsoluteTime insertStart = CFAbsoluteTimeGetCurrent();
    NSString *failure = [client insertText:text];
    if (failure) {
        DWLogEvent(@"catch %lld: released after %.0f ms, insert into %@ failed: %@; key goes through", catchID, waited,
                   bundleID, failure);
    } else {
        DWLogEvent(@"catch %lld: released after %.0f ms, inserted into %@ (%.0f ms); key goes through", catchID, waited,
                   bundleID, MillisecondsSince(insertStart));
    }
    return NO;
}

@end
