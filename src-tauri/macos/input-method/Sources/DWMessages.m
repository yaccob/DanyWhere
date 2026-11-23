#import "DWMessages.h"

NSString *const DWReasonNoClient = @"no_client";
NSString *const DWReasonError = @"error";
const NSInteger DWMaxHoldMs = 10000;
const NSInteger DWDefaultHoldMs = 1500;

static const NSUInteger kDefaultMaxLineLength = 8 * 1024 * 1024;

#pragma mark - JSON writing

NSString *DWJSONQuote(NSString *string) {
    NSMutableString *out = [NSMutableString stringWithCapacity:string.length + 2];
    [out appendString:@"\""];
    NSUInteger length = string.length;
    for (NSUInteger i = 0; i < length; i++) {
        unichar c = [string characterAtIndex:i];
        switch (c) {
            case '"': [out appendString:@"\\\""]; break;
            case '\\': [out appendString:@"\\\\"]; break;
            case '\n': [out appendString:@"\\n"]; break;
            case '\r': [out appendString:@"\\r"]; break;
            case '\t': [out appendString:@"\\t"]; break;
            case '\b': [out appendString:@"\\b"]; break;
            case '\f': [out appendString:@"\\f"]; break;
            default:
                if (c < 0x20) {
                    [out appendFormat:@"\\u%04x", c];
                } else {
                    [out appendFormat:@"%C", c];
                }
        }
    }
    [out appendString:@"\""];
    return out;
}

/// Builds one JSON object with its keys in the order they are added.
@interface DWJSONLine : NSObject
- (instancetype)initWithType:(NSString *)type;
- (void)string:(NSString *)value key:(NSString *)key;
- (void)integer:(long long)value key:(NSString *)key;
- (void)boolean:(BOOL)value key:(NSString *)key;
- (NSData *)data;
@end

@implementation DWJSONLine {
    NSMutableString *_text;
}

- (instancetype)initWithType:(NSString *)type {
    if ((self = [super init])) {
        _text = [NSMutableString stringWithString:@"{\"type\":"];
        [_text appendString:DWJSONQuote(type)];
    }
    return self;
}

- (void)key:(NSString *)key {
    [_text appendString:@","];
    [_text appendString:DWJSONQuote(key)];
    [_text appendString:@":"];
}

- (void)string:(NSString *)value key:(NSString *)key {
    [self key:key];
    [_text appendString:DWJSONQuote(value)];
}

- (void)integer:(long long)value key:(NSString *)key {
    [self key:key];
    [_text appendFormat:@"%lld", value];
}

- (void)boolean:(BOOL)value key:(NSString *)key {
    [self key:key];
    [_text appendString:value ? @"true" : @"false"];
}

- (NSData *)data {
    NSString *line = [_text stringByAppendingString:@"}\n"];
    // Lone UTF-16 surrogates cannot be UTF-8; they never occur in the ids
    // and versions this side sends, but must not lose the whole line.
    return [line dataUsingEncoding:NSUTF8StringEncoding allowLossyConversion:YES];
}

@end

@implementation DWOutgoing

+ (NSData *)helloWithVersion:(NSString *)version pid:(int)pid {
    DWJSONLine *line = [[DWJSONLine alloc] initWithType:@"hello"];
    [line string:version key:@"version"];
    [line integer:pid key:@"pid"];
    return line.data;
}

+ (NSData *)activeWithClient:(NSString *)client {
    DWJSONLine *line = [[DWJSONLine alloc] initWithType:@"active"];
    [line string:client key:@"client"];
    return line.data;
}

+ (NSData *)inactiveWithClient:(NSString *)client {
    DWJSONLine *line = [[DWJSONLine alloc] initWithType:@"inactive"];
    [line string:client key:@"client"];
    return line.data;
}

+ (NSData *)insertedOKWithID:(long long)messageID client:(NSString *)client {
    DWJSONLine *line = [[DWJSONLine alloc] initWithType:@"inserted"];
    [line integer:messageID key:@"id"];
    [line boolean:YES key:@"ok"];
    if (client) [line string:client key:@"client"];
    return line.data;
}

+ (NSData *)insertedFailedWithID:(long long)messageID reason:(NSString *)reason {
    DWJSONLine *line = [[DWJSONLine alloc] initWithType:@"inserted"];
    [line integer:messageID key:@"id"];
    [line boolean:NO key:@"ok"];
    [line string:reason key:@"reason"];
    return line.data;
}

+ (NSData *)caughtWithID:(long long)messageID code:(uint16_t)code mods:(NSUInteger)mods {
    DWJSONLine *line = [[DWJSONLine alloc] initWithType:@"caught"];
    [line integer:messageID key:@"id"];
    [line integer:code key:@"code"];
    [line integer:(long long)mods key:@"mods"];
    return line.data;
}

@end

#pragma mark - Parsing

@implementation DWKeyChord

+ (instancetype)chordWithCode:(uint16_t)code mods:(NSUInteger)mods {
    DWKeyChord *chord = [[self alloc] init];
    chord->_code = code;
    chord->_mods = mods;
    return chord;
}

- (BOOL)isEqual:(id)other {
    if (![other isKindOfClass:[DWKeyChord class]]) return NO;
    DWKeyChord *chord = other;
    return chord.code == _code && chord.mods == _mods;
}

- (NSUInteger)hash {
    return ((NSUInteger)_code << 32) ^ _mods;
}

- (NSString *)description {
    return [NSString stringWithFormat:@"<key %u mods 0x%lx>", _code, (unsigned long)_mods];
}

@end

/// A whole number from JSON (not a bool, not a fraction), else NO.
static BOOL WholeNumber(id value, long long *out) {
    if (![value isKindOfClass:[NSNumber class]]) return NO;
    if (CFGetTypeID((__bridge CFTypeRef)value) == CFBooleanGetTypeID()) return NO;
    NSNumber *number = value;
    const char *type = number.objCType;
    if (strcmp(type, "d") == 0 || strcmp(type, "f") == 0) {
        double d = number.doubleValue;
        if (d != floor(d) || d < -9.2e18 || d > 9.2e18) return NO;
        *out = (long long)d;
        return YES;
    }
    *out = number.longLongValue;
    return YES;
}

@interface DWIncoming ()
@property (nonatomic, readwrite) DWMessageKind kind;
@property (nonatomic, readwrite, copy) NSString *type;
@property (nonatomic, readwrite) long long messageID;
@property (nonatomic, readwrite, copy, nullable) NSString *text;
@property (nonatomic, readwrite, copy, nullable) NSString *source;
@property (nonatomic, readwrite, copy) NSArray<DWKeyChord *> *keys;
@property (nonatomic, readwrite) NSInteger holdMs;
@end

@implementation DWIncoming

static DWMessageKind KindForType(NSString *type) {
    static NSDictionary<NSString *, NSNumber *> *kinds;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        kinds = @{
            @"insert": @(DWMessageInsert),
            @"layout": @(DWMessageLayout),
            @"restore": @(DWMessageRestore),
            @"catch": @(DWMessageCatch),
            @"release": @(DWMessageRelease),
            @"quit": @(DWMessageQuit),
        };
    });
    NSNumber *kind = kinds[type];
    return kind ? (DWMessageKind)kind.integerValue : DWMessageUnknown;
}

+ (instancetype)messageFromLine:(NSData *)line error:(NSString **)error {
#define FAIL(why)                    \
    do {                             \
        if (error) *error = (why);   \
        return nil;                  \
    } while (0)

    NSError *jsonError = nil;
    id object = [NSJSONSerialization JSONObjectWithData:line options:0 error:&jsonError];
    if (![object isKindOfClass:[NSDictionary class]]) FAIL(@"not a JSON object");
    NSDictionary *json = object;
    id type = json[@"type"];
    if (![type isKindOfClass:[NSString class]]) FAIL(@"no string \"type\"");

    DWIncoming *message = [[DWIncoming alloc] init];
    message.type = type;
    message.kind = KindForType(type);
    message.keys = @[];

    long long number = 0;
    switch (message.kind) {
        case DWMessageInsert:
        case DWMessageRelease: {
            if (!WholeNumber(json[@"id"], &number)) FAIL(([NSString stringWithFormat:@"%@ without a numeric \"id\"", type]));
            message.messageID = number;
            id text = json[@"text"];
            if (![text isKindOfClass:[NSString class]]) FAIL(([NSString stringWithFormat:@"%@ without a string \"text\"", type]));
            message.text = text;
            break;
        }
        case DWMessageLayout:
        case DWMessageRestore: {
            id source = json[@"source"];
            if (source == [NSNull null]) {
                message.source = nil;
            } else if ([source isKindOfClass:[NSString class]]) {
                message.source = source;
            } else {
                FAIL(([NSString stringWithFormat:@"%@ without \"source\" (a string or null)", type]));
            }
            break;
        }
        case DWMessageCatch: {
            if (!WholeNumber(json[@"id"], &number)) FAIL(@"catch without a numeric \"id\"");
            message.messageID = number;
            id keys = json[@"keys"];
            if (![keys isKindOfClass:[NSArray class]]) FAIL(@"catch without a \"keys\" list");
            NSMutableArray<DWKeyChord *> *chords = [NSMutableArray array];
            for (id key in (NSArray *)keys) {
                if (![key isKindOfClass:[NSDictionary class]]) FAIL(@"catch key is not an object");
                long long code = 0, mods = 0;
                if (!WholeNumber(key[@"code"], &code) || code < 0 || code > UINT16_MAX) FAIL(@"catch key without a key code");
                id modsValue = key[@"mods"];
                if (modsValue && !WholeNumber(modsValue, &mods)) FAIL(@"catch key with non-numeric \"mods\"");
                if (mods < 0) FAIL(@"catch key with negative \"mods\"");
                [chords addObject:[DWKeyChord chordWithCode:(uint16_t)code mods:(NSUInteger)mods]];
            }
            message.keys = chords;
            long long hold = DWDefaultHoldMs;
            id holdValue = json[@"hold_ms"];
            if (holdValue && holdValue != [NSNull null] && !WholeNumber(holdValue, &hold)) FAIL(@"catch with non-numeric \"hold_ms\"");
            message.holdMs = (NSInteger)MAX(0LL, MIN((long long)DWMaxHoldMs, hold));
            break;
        }
        case DWMessageQuit:
        case DWMessageUnknown:
            break;
    }
    return message;
#undef FAIL
}

@end

#pragma mark - Framing

@implementation DWLineBuffer {
    NSMutableData *_pending;
    NSUInteger _maxLineLength;
    BOOL _discarding; // inside a line that is too long: drop up to its newline
}

- (instancetype)initWithMaxLineLength:(NSUInteger)maxLineLength {
    if ((self = [super init])) {
        _pending = [NSMutableData data];
        _maxLineLength = maxLineLength;
    }
    return self;
}

- (instancetype)init {
    return [self initWithMaxLineLength:kDefaultMaxLineLength];
}

- (NSArray<NSData *> *)linesByAppending:(NSData *)data {
    NSMutableArray<NSData *> *lines = [NSMutableArray array];
    const uint8_t *bytes = data.bytes;
    NSUInteger length = data.length;
    NSUInteger start = 0;
    while (start < length) {
        const uint8_t *newline = memchr(bytes + start, '\n', length - start);
        NSUInteger end = newline ? (NSUInteger)(newline - bytes) : length;
        if (!_discarding) {
            [_pending appendBytes:bytes + start length:end - start];
            if (_pending.length > _maxLineLength) {
                [_pending setLength:0];
                _discarding = YES;
                _droppedLines++;
            }
        }
        if (!newline) break;
        if (!_discarding && _pending.length > 0) {
            [lines addObject:[_pending copy]];
        }
        [_pending setLength:0];
        _discarding = NO;
        start = end + 1;
    }
    return lines;
}

- (void)reset {
    [_pending setLength:0];
    _discarding = NO;
}

@end
