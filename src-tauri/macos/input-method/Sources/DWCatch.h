// Stage 2 (see ../PROTOCOL.md, `catch` and `release`): the app asks the
// input method to catch the next press of a key (e.g. Enter as an end
// shortcut) and to hold it until the dictation's text is there.

#import <Foundation/Foundation.h>

#import "DWMessages.h"

NS_ASSUME_NONNULL_BEGIN

/// The modifier flags a key chord is matched on: Shift, Control, Option and
/// Command out of NSEvent's device-independent flags. Caps Lock, the numeric
/// pad, Help and Function are states of the key or keyboard, not part of a
/// chord, and the device-dependent low bits (left/right) are dropped.
NSUInteger DWChordModifiers(NSUInteger modifierFlags);

/// Which keys to catch next. Used on the main thread only.
@interface DWCatchState : NSObject
@property (nonatomic, readonly) BOOL armed;
@property (nonatomic, readonly) long long armedID;
@property (nonatomic, readonly) NSInteger holdMs;
/// Arms for one press of any of `keys`; an empty list disarms.
- (void)armWithID:(long long)catchID keys:(NSArray<DWKeyChord *> *)keys holdMs:(NSInteger)holdMs;
- (void)disarm;
/// Called for every key-down. YES when this press is the one to catch: one
/// of the keys, not an auto-repeat, and a text field has the input method.
/// A caught press disarms (one press per `catch`).
- (BOOL)takeKeyCode:(uint16_t)code modifierFlags:(NSUInteger)flags isRepeat:(BOOL)isRepeat hasClient:(BOOL)hasClient;
@end

typedef NS_ENUM(NSInteger, DWHoldOutcome) {
    DWHoldReleased, // `release` came in time: insert its text, let the key through
    DWHoldTimedOut, // no `release` in time: swallow the key
};

/// Where a held key waits for its `release`. The main thread waits inside
/// IMK's handleEvent:client:, so `release` arrives on the link's own queue:
/// thread-safe.
@interface DWHoldGate : NSObject
@property (atomic, readonly) BOOL holding;
/// Starts holding for `catchID`; call before `caught` is sent.
- (void)beginForID:(long long)catchID;
/// Hands the text of `release` to the hold waiting for `catchID`. NO when no
/// hold for that id waits (too late, or a wrong id): the release is dropped.
- (BOOL)releaseID:(long long)catchID text:(NSString *)text;
/// Waits up to `ms` for the release and ends the hold either way.
- (DWHoldOutcome)waitMilliseconds:(NSInteger)ms text:(NSString *_Nullable *_Nullable)text;
@end

NS_ASSUME_NONNULL_END
