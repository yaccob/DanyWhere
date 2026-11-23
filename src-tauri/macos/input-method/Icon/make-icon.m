// Draws the input menu icon of DanyWhere's input method: a bold "D" in a
// rounded square outline, black on transparent. It is a template image
// (TISIconIsTemplate), so macOS tints it for light and dark menu bars.
// DanyWhere's own design; nothing of Handy's. Writes one TIFF with a 16 px
// and a 32 px representation of the same 16 pt image.
//
//   build.sh --make-icon       (writes Icon/DanyWhereInput.tiff)
//   make-icon <output.tiff>

#import <AppKit/AppKit.h>

static NSBitmapImageRep *Representation(NSInteger pixels) {
    NSBitmapImageRep *rep = [[NSBitmapImageRep alloc] initWithBitmapDataPlanes:NULL
                                                                    pixelsWide:pixels
                                                                    pixelsHigh:pixels
                                                                 bitsPerSample:8
                                                               samplesPerPixel:4
                                                                      hasAlpha:YES
                                                                      isPlanar:NO
                                                                colorSpaceName:NSDeviceRGBColorSpace
                                                                   bytesPerRow:0
                                                                  bitsPerPixel:0];
    rep.size = NSMakeSize(16, 16);
    [NSGraphicsContext saveGraphicsState];
    // The context maps the rep's size in points (16) onto its pixels.
    NSGraphicsContext.currentContext = [NSGraphicsContext graphicsContextWithBitmapImageRep:rep];

    [[NSColor blackColor] setStroke];
    NSBezierPath *frame = [NSBezierPath bezierPathWithRoundedRect:NSMakeRect(1.25, 1.25, 13.5, 13.5) xRadius:3 yRadius:3];
    frame.lineWidth = 1.3;
    [frame stroke];

    NSDictionary *attributes = @{
        NSFontAttributeName: [NSFont systemFontOfSize:10.5 weight:NSFontWeightHeavy],
        NSForegroundColorAttributeName: [NSColor blackColor],
    };
    NSAttributedString *letter = [[NSAttributedString alloc] initWithString:@"D" attributes:attributes];
    NSSize size = letter.size;
    [letter drawAtPoint:NSMakePoint((16 - size.width) / 2, (16 - size.height) / 2 + 0.3)];

    [NSGraphicsContext restoreGraphicsState];
    return rep;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 2) {
            fprintf(stderr, "usage: make-icon <output.tiff>\n");
            return 2;
        }
        NSImage *image = [[NSImage alloc] initWithSize:NSMakeSize(16, 16)];
        [image addRepresentation:Representation(16)];
        [image addRepresentation:Representation(32)];
        NSData *tiff = [image TIFFRepresentationUsingCompression:NSTIFFCompressionLZW factor:0];
        if (!tiff) {
            fprintf(stderr, "no TIFF representation\n");
            return 1;
        }
        NSString *path = [NSString stringWithUTF8String:argv[1]];
        if (![tiff writeToFile:path atomically:YES]) {
            fprintf(stderr, "cannot write %s\n", argv[1]);
            return 1;
        }
    }
    return 0;
}
