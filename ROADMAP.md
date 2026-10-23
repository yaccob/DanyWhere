# DanyWhere roadmap

Planned work on DanyWhere, roughly in priority order within each section. Items that would benefit every Handy user are marked **upstream candidate** and are offered to [Handy](https://github.com/cjpais/Handy) as well.

## Features and improvements

### Keep the transcript when there is nowhere to paste it

**Problem:** when the focus is not on something that accepts text (the desktop, a non-editable window), recording and transcription work, but the text ends up nowhere.

**Cause (inherited from Handy):** pasting saves the clipboard, writes the transcript, sends the paste shortcut without checking for an editable target, then restores the old clipboard (`paste_via_clipboard` in `src-tauri/src/clipboard.rs`). With no target, the paste goes nowhere and the restore also removes the transcript from the clipboard. It is only recoverable through the history or the tray item "Copy last transcription".

**Goal:** if the transcript could not be pasted, leave it on the clipboard and, once transcription (and post-processing, if used) has finished, show a notification: the text is on the clipboard because it could not be pasted — no editable field had focus.

**Approach:** Handy's experimental receipt-based paste (`reliable_paste`, `src-tauri/src/paste_tx/macos.rs`) puts a promise on the pasteboard and gets a callback when an app actually reads it; it already detects "no read within timeout". That signal is a more reliable trigger than guessing whether the focused element is editable. Clipboard managers that read the pasteboard eagerly need checking (Handy already marks the transcript as concealed for them).

**Upstream candidate.**

### Continue without the Accessibility permission

Done in DanyWhere's base: onboarding offers "Continue without Accessibility" for managed Macs (shortcuts via the Tauri backend, transcript left on the clipboard). **Upstream candidate** (needs a Handy Discussion first because of the feature freeze).

## Branding

- A real DanyWhere wordmark to replace the placeholder text.
- Own tray icons for the recording and transcribing states (currently Handy's ear and brain).
- About page: "DanyWhere is based on Handy" with a link.

## Distribution

- Release workflow: build, sign the updater artifacts, publish `latest.json` in this repository's releases (the updater already points here).
- Trademark search for the name before going public.
- Apple Developer ID signing and notarization, so others can install without Gatekeeper warnings.
- Windows signing with an own certificate (Handy's signing setup was removed).
