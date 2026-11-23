// What the input method needs from macOS's input sources (Text Input
// Sources), behind a protocol so the core can be tested with a fake.

#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

@protocol DWSystem <NSObject>
/// TISInputSourceID of the current keyboard input source.
- (nullable NSString *)currentInputSourceID;
/// TISSelectInputSource on the source with that id.
- (BOOL)selectInputSourceWithID:(NSString *)sourceID reason:(NSString *_Nullable *_Nullable)reason;
/// TISSetInputMethodKeyboardLayoutOverride with the keyboard layout of that
/// id; nil sets the most recently used ASCII-capable layout, which is what
/// macOS uses without an override.
- (BOOL)setKeyboardLayoutOverrideWithID:(nullable NSString *)sourceID reason:(NSString *_Nullable *_Nullable)reason;
@end

/// The real one, on Text Input Sources. Main thread only.
@interface DWSystemTIS : NSObject <DWSystem>
@end

NS_ASSUME_NONNULL_END
