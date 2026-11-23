#import "DWInputController.h"

#import "DWLog.h"

@implementation DanyWhereInputController

- (void)activateServer:(id)sender {
    [super activateServer:sender];
    [DWCore.shared clientActivated:self];
}

- (void)deactivateServer:(id)sender {
    [DWCore.shared clientDeactivated:self];
    [super deactivateServer:sender];
}

- (void)inputControllerWillClose {
    [DWCore.shared clientClosed:self];
    [super inputControllerWillClose];
}

- (NSUInteger)recognizedEvents:(id)sender {
    return NSEventMaskKeyDown;
}

/// NO lets the key through to the app unchanged; only a key caught for
/// stage 2 can make this return YES (swallowed).
- (BOOL)handleEvent:(NSEvent *)event client:(id)sender {
    if (event.type != NSEventTypeKeyDown) return NO;
    DWCore *core = DWCore.shared;
    if (!core) return NO;
    return [core handleKeyDownWithCode:event.keyCode
                         modifierFlags:event.modifierFlags
                              isRepeat:event.isARepeat
                                client:self];
}

#pragma mark - DWTextClient

- (NSString *)clientBundleID {
    id<IMKTextInput> client = [self client];
    if (!client) return nil;
    @try {
        return [client bundleIdentifier];
    } @catch (NSException *exception) {
        return nil;
    }
}

- (NSString *)insertText:(NSString *)text {
    id<IMKTextInput> client = [self client];
    if (!client) return DWReasonNoClient;
    @try {
        [client insertText:text replacementRange:NSMakeRange(NSNotFound, 0)];
    } @catch (NSException *exception) {
        DWLogEvent(@"insertText raised %@", exception.name);
        return DWReasonError;
    }
    return nil;
}

@end
