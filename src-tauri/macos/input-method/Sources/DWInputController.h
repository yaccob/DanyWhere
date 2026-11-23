// The InputMethodKit controller: one per text field that has the input
// method. Thin glue: everything it learns goes to DWCore.shared.

#import <Cocoa/Cocoa.h>
#import <InputMethodKit/InputMethodKit.h>

#import "DWCore.h"

/// Named in Info.plist (InputMethodServerControllerClass).
@interface DanyWhereInputController : IMKInputController <DWTextClient>
@end
