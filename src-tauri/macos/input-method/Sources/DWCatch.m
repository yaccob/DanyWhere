#import "DWCatch.h"

// NSEventModifierFlagShift, …Control, …Option, …Command (AppKit's values,
// spelled out so the logic needs no AppKit).
static const NSUInteger kChordMask = (1UL << 17) | (1UL << 18) | (1UL << 19) | (1UL << 20);

NSUInteger DWChordModifiers(NSUInteger modifierFlags) {
    return modifierFlags & kChordMask;
}

@implementation DWCatchState {
    NSArray<DWKeyChord *> *_keys;
}

- (void)armWithID:(long long)catchID keys:(NSArray<DWKeyChord *> *)keys holdMs:(NSInteger)holdMs {
    if (keys.count == 0) {
        [self disarm];
        return;
    }
    _keys = [keys copy];
    _armedID = catchID;
    _holdMs = holdMs;
    _armed = YES;
}

- (void)disarm {
    _keys = nil;
    _armed = NO;
}

- (BOOL)takeKeyCode:(uint16_t)code modifierFlags:(NSUInteger)flags isRepeat:(BOOL)isRepeat hasClient:(BOOL)hasClient {
    if (!_armed || isRepeat || !hasClient) return NO;
    NSUInteger mods = DWChordModifiers(flags);
    for (DWKeyChord *key in _keys) {
        if (key.code == code && DWChordModifiers(key.mods) == mods) {
            [self disarm];
            return YES;
        }
    }
    return NO;
}

@end

@implementation DWHoldGate {
    NSCondition *_condition;
    long long _pendingID;
    NSString *_text; // set by a matching release
}

@synthesize holding = _holding;

- (instancetype)init {
    if ((self = [super init])) {
        _condition = [[NSCondition alloc] init];
    }
    return self;
}

- (BOOL)holding {
    [_condition lock];
    BOOL holding = _holding;
    [_condition unlock];
    return holding;
}

- (void)beginForID:(long long)catchID {
    [_condition lock];
    _pendingID = catchID;
    _text = nil;
    _holding = YES;
    [_condition unlock];
}

- (BOOL)releaseID:(long long)catchID text:(NSString *)text {
    [_condition lock];
    BOOL taken = _holding && _pendingID == catchID && _text == nil;
    if (taken) {
        _text = [text copy];
        [_condition broadcast];
    }
    [_condition unlock];
    return taken;
}

- (DWHoldOutcome)waitMilliseconds:(NSInteger)ms text:(NSString **)text {
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:MAX(0, ms) / 1000.0];
    [_condition lock];
    while (_text == nil) {
        if (![_condition waitUntilDate:deadline]) break;
    }
    NSString *released = _text;
    _text = nil;
    _holding = NO;
    [_condition unlock];
    if (text) *text = released;
    return released ? DWHoldReleased : DWHoldTimedOut;
}

@end
