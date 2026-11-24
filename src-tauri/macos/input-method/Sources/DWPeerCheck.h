// The peer check of the link (see ../PROTOCOL.md, "The link"): before
// anything else is read or written, each side checks that the process at the
// other end of the socket is signed with the expected identifier and with
// the same leaf certificate as itself.

#import <Foundation/Foundation.h>
#import <Security/Security.h>

NS_ASSUME_NONNULL_BEGIN

/// Checks the peer of a connected socket. Returns NO and a reason when the
/// peer may not talk to this side.
typedef BOOL (^DWPeerCheck)(int fd, NSString *_Nullable *_Nonnull reason);

/// Lowercase hex digits of `data`.
NSString *DWHexString(NSData *data);

/// `identifier "<expectedID>" and certificate leaf = H"<sha1 hex>"`.
NSString *DWRequirementString(NSString *expectedID, NSData *leafSHA1);

/// SHA-1 of the leaf certificate `code` is signed with; nil when it has none
/// (ad hoc or unsigned).
NSData *_Nullable DWLeafCertificateSHA1OfStaticCode(SecStaticCodeRef code);

/// SHA-1 of this process's own leaf certificate (computed once); nil when
/// its code has no certificate.
NSData *_Nullable DWOwnLeafCertificateSHA1(void);

/// The real check: the peer's audit token (LOCAL_PEERTOKEN) →
/// SecCodeCopyGuestWithAttributes → SecCodeCheckValidity against
/// DWRequirementString(expectedID, ownLeafSHA1). With `ownLeafSHA1` nil this
/// side has no certificate and accepts no peer.
BOOL DWCheckPeer(int fd, NSString *expectedID, NSData *_Nullable ownLeafSHA1,
                 NSString *_Nullable *_Nullable reason);

/// DWCheckPeer with this process's own leaf certificate.
DWPeerCheck DWMakeSignaturePeerCheck(NSString *expectedID);

NS_ASSUME_NONNULL_END
