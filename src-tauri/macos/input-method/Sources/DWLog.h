// The input method's log, `~/Library/Logs/<app id>/input-method.log`.
// Events only: never typed keys, never inserted text.

#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

@interface DWLog : NSObject
+ (instancetype)shared;
/// Appends to `path` from now on (nil: logs nowhere). Once the file would
/// grow past `maxBytes`, it moves to `<path>.1` (replacing an older one) and
/// a new file starts.
- (void)configureWithPath:(nullable NSString *)path maxBytes:(unsigned long long)maxBytes;
- (void)event:(NSString *)message;
/// Waits until every event so far is written.
- (void)flush;
@end

/// Logs one event line to DWLog.shared.
void DWLogEvent(NSString *format, ...) NS_FORMAT_FUNCTION(1, 2);

NS_ASSUME_NONNULL_END
