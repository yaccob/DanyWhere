// The input method's end of the link to DanyWhere (see ../PROTOCOL.md, "The
// link"): a Unix domain stream socket the app listens on. All socket work
// runs on the link's own serial queue.

#import <Foundation/Foundation.h>

#import "DWMessages.h"
#import "DWPeerCheck.h"

NS_ASSUME_NONNULL_BEGIN

@class DWLink;

@protocol DWLinkDelegate <NSObject>
/// A verified connection is up and `hello` is sent. On the delivery queue.
- (void)linkDidConnect:(DWLink *)link;
/// A connection that was up is gone. On the delivery queue.
- (void)linkDidDisconnect:(DWLink *)link reason:(NSString *)reason;
/// Every known message except `release`. On the delivery queue.
- (void)link:(DWLink *)link didReceiveMessage:(DWIncoming *)message;
/// `release`, on the link's own queue: the main thread may be waiting
/// inside handleEvent:client: for exactly this message.
- (void)link:(DWLink *)link didReceiveRelease:(DWIncoming *)message;
@end

@interface DWLink : NSObject
- (instancetype)initWithSocketPath:(NSString *)path
                             hello:(NSData *)helloLine
                         peerCheck:(DWPeerCheck)peerCheck NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

@property (nonatomic, readonly, copy) NSString *socketPath;
@property (atomic, weak, nullable) id<DWLinkDelegate> delegate;
/// Where the delegate hears of everything but `release`; the main queue by default.
@property (atomic, strong) dispatch_queue_t deliveryQueue;
@property (atomic, readonly) BOOL connected;

/// Connects in the background unless connected: checks the peer before
/// anything else, then sends `hello`. A failure is logged when its reason
/// differs from the previous attempt's.
- (void)connect;
/// Sends one line in the background; dropped while not connected.
- (void)send:(NSData *)line;
/// Closes the connection in the background (the delegate hears of it).
- (void)disconnect;
/// Runs `block` on the link's queue after everything queued before it.
- (void)sync:(dispatch_block_t)block;
@end

NS_ASSUME_NONNULL_END
