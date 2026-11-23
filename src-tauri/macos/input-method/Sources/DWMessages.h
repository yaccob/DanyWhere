// Messages between DanyWhere and its input method (see ../PROTOCOL.md):
// newline-delimited JSON objects in UTF-8, each with a "type". Unknown types
// and unknown fields are ignored, so either side can be newer.

#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// The reasons of a failed `inserted`.
extern NSString *const DWReasonNoClient; // "no_client": no text field has the input method
extern NSString *const DWReasonError;    // "error"

/// The longest `hold_ms` a `catch` may ask for. The app in front waits while
/// a key is held, so a larger value is cut down to this.
extern const NSInteger DWMaxHoldMs;
/// `hold_ms` of a `catch` that names none.
extern const NSInteger DWDefaultHoldMs;

typedef NS_ENUM(NSInteger, DWMessageKind) {
    DWMessageUnknown = 0, // a type this input method does not know: ignored
    DWMessageInsert,
    DWMessageLayout,
    DWMessageRestore,
    DWMessageCatch,
    DWMessageRelease,
    DWMessageQuit,
};

/// A key to catch in stage 2: a virtual key code and device-independent
/// modifier flags (as `DWChordModifiers` keeps them).
@interface DWKeyChord : NSObject
@property (nonatomic, readonly) uint16_t code;
@property (nonatomic, readonly) NSUInteger mods;
+ (instancetype)chordWithCode:(uint16_t)code mods:(NSUInteger)mods;
@end

/// A message from the app to the input method.
@interface DWIncoming : NSObject
@property (nonatomic, readonly) DWMessageKind kind;
/// The message's "type" as sent, also for unknown kinds.
@property (nonatomic, readonly, copy) NSString *type;
/// "id" of insert, catch and release.
@property (nonatomic, readonly) long long messageID;
/// "text" of insert and release.
@property (nonatomic, readonly, copy, nullable) NSString *text;
/// "source" of layout and restore; nil when the app sent null (clear).
@property (nonatomic, readonly, copy, nullable) NSString *source;
/// "keys" of catch; empty disarms.
@property (nonatomic, readonly, copy) NSArray<DWKeyChord *> *keys;
/// "hold_ms" of catch, limited to 0...DWMaxHoldMs.
@property (nonatomic, readonly) NSInteger holdMs;

/// Parses one line (without its newline). Returns nil and a reason for a
/// line that is no JSON object, has no string "type", or lacks a field its
/// known type needs; returns a message of kind DWMessageUnknown for an
/// unknown type. Unknown fields are ignored.
+ (nullable instancetype)messageFromLine:(NSData *)line error:(NSString *_Nullable *_Nullable)error;
@end

/// Messages from the input method to the app, each one line ending in "\n",
/// with the keys in the order PROTOCOL.md shows them.
@interface DWOutgoing : NSObject
+ (NSData *)helloWithVersion:(NSString *)version pid:(int)pid;
+ (NSData *)activeWithClient:(NSString *)client;
+ (NSData *)inactiveWithClient:(NSString *)client;
+ (NSData *)insertedOKWithID:(long long)messageID client:(nullable NSString *)client;
+ (NSData *)insertedFailedWithID:(long long)messageID reason:(NSString *)reason;
+ (NSData *)caughtWithID:(long long)messageID code:(uint16_t)code mods:(NSUInteger)mods;
@end

/// Splits a byte stream into lines at "\n". Keeps a partial line until its
/// newline arrives, drops empty lines, and drops a line longer than its limit
/// as a whole (counted in `droppedLines`), so a broken peer cannot fill memory.
@interface DWLineBuffer : NSObject
- (instancetype)initWithMaxLineLength:(NSUInteger)maxLineLength NS_DESIGNATED_INITIALIZER;
- (instancetype)init; // limit 8 MiB
/// Appends bytes and returns the lines completed by them, without "\n".
- (NSArray<NSData *> *)linesByAppending:(NSData *)data;
@property (nonatomic, readonly) NSUInteger droppedLines;
- (void)reset;
@end

/// JSON string literal for `string` (quotes included). Escapes '"', '\\' and
/// control characters only; everything else stays as UTF-8.
NSString *DWJSONQuote(NSString *string);

NS_ASSUME_NONNULL_END
