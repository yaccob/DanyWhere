// What the input method does, apart from InputMethodKit and the socket: it
// tracks the text field that has the input method, answers the app's
// messages and catches keys for stage 2 (see ../PROTOCOL.md). Main thread
// only (the link's delivery queue), except where noted.

#import <Foundation/Foundation.h>

#import "DWCatch.h"
#import "DWLink.h"
#import "DWSystem.h"

NS_ASSUME_NONNULL_BEGIN

/// A text field that has the input method (an IMK client behind its controller).
@protocol DWTextClient <NSObject>
/// Bundle id of the app the text field belongs to.
@property (nonatomic, readonly, copy, nullable) NSString *clientBundleID;
/// Inserts at the insertion point; nil on success, else DWReasonNoClient or DWReasonError.
- (nullable NSString *)insertText:(NSString *)text;
@end

@interface DWCore : NSObject <DWLinkDelegate>
/// The instance InputMethodKit's controllers talk to.
@property (class, atomic, strong, nullable) DWCore *shared;

- (instancetype)initWithInputSourceID:(NSString *)inputSourceID
                                 link:(DWLink *)link
                               system:(id<DWSystem>)system NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

@property (nonatomic, readonly, copy) NSString *inputSourceID;
@property (nonatomic, readonly) DWLink *link;
/// The text field of the last activateServer, until its deactivateServer.
@property (nonatomic, readonly, nullable) id<DWTextClient> currentClient;
/// From `layout`; nil = no override.
@property (nonatomic, readonly, copy, nullable) NSString *layoutSource;
/// From `restore`; nil = none.
@property (nonatomic, readonly, copy, nullable) NSString *restoreSource;
@property (nonatomic, readonly) DWCatchState *catchState;
@property (nonatomic, readonly) DWHoldGate *holdGate;
/// How long an `insert` without a text field waits for one before it answers
/// `no_client` (default 0.5 s): the app may send it right after selecting the
/// source, before macOS has activated the input method for the field.
@property (nonatomic) NSTimeInterval clientWait;
/// Called for `quit` after logging; exits the process by default.
@property (nonatomic, copy) void (^quitHandler)(void);

/// Connects at launch and every 2 s while this input source is selected and
/// not connected.
- (void)start;
/// One step of the 2 s timer.
- (void)tick;

/// activateServer: a text field now has the input method.
- (void)clientActivated:(id<DWTextClient>)client;
/// deactivateServer.
- (void)clientDeactivated:(id<DWTextClient>)client;
/// inputControllerWillClose: forget the client without a message.
- (void)clientClosed:(id<DWTextClient>)client;

/// handleEvent:client: for a key-down. Returns what IMK should get: NO lets
/// the key through, YES swallows it. Outside a stage-2 catch always NO.
- (BOOL)handleKeyDownWithCode:(uint16_t)code
                modifierFlags:(NSUInteger)flags
                     isRepeat:(BOOL)isRepeat
                       client:(nullable id<DWTextClient>)client;
@end

NS_ASSUME_NONNULL_END
