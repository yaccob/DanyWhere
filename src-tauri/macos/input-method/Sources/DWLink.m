#import "DWLink.h"

#import "DWLog.h"

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

/// How long one write may wait for the app to read before the link counts
/// as broken.
static const int kWriteTimeoutMs = 2000;

@interface DWLink ()
@property (atomic, readwrite) BOOL connected;
@end

@implementation DWLink {
    dispatch_queue_t _queue;
    DWPeerCheck _peerCheck;
    NSData *_hello;
    // Only touched on _queue:
    int _fd;
    dispatch_source_t _readSource;
    DWLineBuffer *_lines;
    NSString *_lastFailure;
}

- (instancetype)initWithSocketPath:(NSString *)path hello:(NSData *)helloLine peerCheck:(DWPeerCheck)peerCheck {
    if ((self = [super init])) {
        _socketPath = [path copy];
        _hello = [helloLine copy];
        _peerCheck = [peerCheck copy];
        _queue = dispatch_queue_create("danywhere.inputmethod.link", DISPATCH_QUEUE_SERIAL);
        _deliveryQueue = dispatch_get_main_queue();
        _fd = -1;
        _lines = [[DWLineBuffer alloc] init];
    }
    return self;
}

- (void)dealloc {
    if (_readSource) dispatch_source_cancel(_readSource);
}

- (void)connect {
    dispatch_async(_queue, ^{
        [self connectOnQueue];
    });
}

- (void)send:(NSData *)line {
    NSData *copy = [line copy];
    dispatch_async(_queue, ^{
        if (self->_fd < 0) return;
        if (![self writeAll:copy]) [self closeWithReason:@"write failed or timed out"];
    });
}

- (void)disconnect {
    dispatch_async(_queue, ^{
        if (self->_fd >= 0) [self closeWithReason:@"closed by the input method"];
    });
}

- (void)sync:(dispatch_block_t)block {
    dispatch_sync(_queue, block);
}

#pragma mark - On the link's queue

/// Logs a failed attempt only when its reason changed, since attempts
/// repeat every 2 s while DanyWhere is not running.
- (void)failedToConnect:(NSString *)reason {
    if ([reason isEqualToString:_lastFailure]) return;
    _lastFailure = [reason copy];
    DWLogEvent(@"link: not connected: %@", reason);
}

- (void)connectOnQueue {
    if (_fd >= 0) return;
    const char *path = _socketPath.fileSystemRepresentation;
    struct sockaddr_un address;
    memset(&address, 0, sizeof address);
    if (strlen(path) >= sizeof address.sun_path) {
        [self failedToConnect:@"socket path too long"];
        return;
    }
    address.sun_family = AF_UNIX;
    strlcpy(address.sun_path, path, sizeof address.sun_path);
    address.sun_len = (unsigned char)SUN_LEN(&address);

    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        [self failedToConnect:[NSString stringWithFormat:@"socket: %s", strerror(errno)]];
        return;
    }
    int on = 1;
    setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, sizeof on);
    fcntl(fd, F_SETFD, FD_CLOEXEC);
    if (connect(fd, (struct sockaddr *)&address, sizeof address) != 0) {
        int error = errno;
        close(fd);
        [self failedToConnect:[NSString stringWithFormat:@"connect: %s", strerror(error)]];
        return;
    }

    // Nothing is read or written before the peer is known to be DanyWhere.
    NSString *reason = nil;
    if (!_peerCheck(fd, &reason)) {
        close(fd);
        _lastFailure = nil; // always log a refused peer
        DWLogEvent(@"link: peer check failed, connection closed: %@", reason ?: @"?");
        return;
    }
    fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK);
    _fd = fd;
    [_lines reset];
    if (![self writeAll:_hello]) {
        close(fd);
        _fd = -1;
        DWLogEvent(@"link: peer verified, but hello could not be sent");
        return;
    }
    _lastFailure = nil;
    DWLogEvent(@"link: connected to %@, peer verified, hello sent", _socketPath.lastPathComponent);

    dispatch_source_t source = dispatch_source_create(DISPATCH_SOURCE_TYPE_READ, (uintptr_t)fd, 0, _queue);
    __weak DWLink *weakSelf = self;
    dispatch_source_set_event_handler(source, ^{
        [weakSelf readAvailable];
    });
    dispatch_source_set_cancel_handler(source, ^{
        close(fd);
    });
    _readSource = source;
    dispatch_resume(source);
    self.connected = YES;

    dispatch_async(self.deliveryQueue, ^{
        [self.delegate linkDidConnect:self];
    });
}

- (BOOL)writeAll:(NSData *)data {
    const uint8_t *bytes = data.bytes;
    size_t left = data.length;
    while (left > 0) {
        ssize_t n = write(_fd, bytes, left);
        if (n > 0) {
            bytes += n;
            left -= (size_t)n;
            continue;
        }
        if (n < 0 && errno == EINTR) continue;
        if (n < 0 && errno == EAGAIN) {
            struct pollfd p = {.fd = _fd, .events = POLLOUT};
            int ready = poll(&p, 1, kWriteTimeoutMs);
            if (ready > 0) continue;
            if (ready < 0 && errno == EINTR) continue;
        }
        return NO;
    }
    return YES;
}

- (void)readAvailable {
    if (_fd < 0) return;
    uint8_t buffer[64 * 1024];
    while (YES) {
        ssize_t n = read(_fd, buffer, sizeof buffer);
        if (n > 0) {
            NSData *chunk = [NSData dataWithBytesNoCopy:buffer length:(NSUInteger)n freeWhenDone:NO];
            for (NSData *line in [_lines linesByAppending:chunk]) [self handleLine:line];
            if (_fd < 0) return;
            if ((size_t)n < sizeof buffer) return;
            continue;
        }
        if (n == 0) {
            [self closeWithReason:@"closed by DanyWhere"];
            return;
        }
        if (errno == EINTR) continue;
        if (errno == EAGAIN) return;
        [self closeWithReason:[NSString stringWithFormat:@"read: %s", strerror(errno)]];
        return;
    }
}

- (void)handleLine:(NSData *)line {
    NSString *error = nil;
    DWIncoming *message = [DWIncoming messageFromLine:line error:&error];
    if (!message) {
        DWLogEvent(@"link: skipped an invalid message (%@)", error);
        return;
    }
    switch (message.kind) {
        case DWMessageUnknown:
            DWLogEvent(@"link: ignored a message of unknown type \"%@\"", message.type);
            return;
        case DWMessageRelease:
            // Not through the delivery queue: the main thread may be the one
            // waiting for it inside handleEvent:client:.
            [self.delegate link:self didReceiveRelease:message];
            return;
        default: {
            dispatch_async(self.deliveryQueue, ^{
                [self.delegate link:self didReceiveMessage:message];
            });
            return;
        }
    }
}

- (void)closeWithReason:(NSString *)reason {
    if (_fd < 0) return;
    if (_readSource) {
        dispatch_source_cancel(_readSource); // its cancel handler closes the fd
        _readSource = nil;
    } else {
        close(_fd);
    }
    _fd = -1;
    [_lines reset];
    self.connected = NO;
    DWLogEvent(@"link: disconnected: %@", reason);
    dispatch_async(self.deliveryQueue, ^{
        [self.delegate linkDidDisconnect:self reason:reason];
    });
}

@end
