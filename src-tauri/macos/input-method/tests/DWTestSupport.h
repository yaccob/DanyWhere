// Test doubles: a fake DanyWhere listening on a socket, a fake text field,
// fake input sources.

#import <Foundation/Foundation.h>

#import "DWCore.h"
#import "DWSystem.h"

NS_ASSUME_NONNULL_BEGIN

/// A short, unique socket path in the temp dir (removed if it exists).
NSString *DWTestSocketPath(NSString *tag);

/// Two connected ends of a Unix stream socket made with listen/connect, so
/// the kernel records the peer's credentials as it does for the real link.
@interface DWSocketPair : NSObject
+ (nullable instancetype)connectedPair;
@property (nonatomic, readonly) int serverFD;
@property (nonatomic, readonly) int clientFD;
- (void)close;
@end

/// A fake DanyWhere: listens, accepts one connection at a time, reads and
/// writes lines. Blocking calls with timeouts, for the test thread.
@interface DWTestServer : NSObject
- (nullable instancetype)initWithPath:(NSString *)path;
@property (nonatomic, readonly, copy) NSString *path;
/// Waits for the next connection; replaces the current one.
- (BOOL)acceptWithin:(NSTimeInterval)timeout;
/// The next line (without "\n"), or nil after the timeout or at EOF.
- (nullable NSString *)readLineWithin:(NSTimeInterval)timeout;
/// YES when the client closed its end (EOF) within the timeout with nothing
/// (more) to read.
- (BOOL)clientClosedWithin:(NSTimeInterval)timeout;
- (BOOL)sendLine:(NSString *)line;
/// Sends bytes as they are, without a newline.
- (BOOL)sendRaw:(NSString *)text;
/// Closes the accepted connection.
- (void)dropClient;
/// Closes the listener and removes the socket file.
- (void)stop;
@end

/// A fake text field.
@interface DWFakeClient : NSObject <DWTextClient>
- (instancetype)initWithBundleID:(nullable NSString *)bundleID;
@property (nonatomic, copy, nullable) NSString *clientBundleID;
/// What insertText: returns (nil = success).
@property (nonatomic, copy, nullable) NSString *failure;
@property (atomic, readonly) NSArray<NSString *> *inserted;
@property (atomic, readonly) int insertCalls;
@end

/// Fake input sources: records what was selected and overridden.
@interface DWFakeSystem : NSObject <DWSystem>
@property (atomic, copy, nullable) NSString *current;
@property (atomic, readonly) NSArray<NSString *> *selected;
/// Every override set; NSNull for "back to the default".
@property (atomic, readonly) NSArray *overrides;
@end

NS_ASSUME_NONNULL_END
