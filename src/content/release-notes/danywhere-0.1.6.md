## The second press without Accessibility, in more apps

On a Mac without the Accessibility permission, the second press of a dictation shortcut ends the dictation and inserts it through a macOS service. It now also works in two cases where it did nothing before. Nothing changes on Windows and Linux.

- **Control instead of Option.** A shortcut with Option (⌥) but neither Control (⌃) nor Command (⌘) never reaches a service, because Option with a key types a character. New installations therefore start with **⌃Space** to dictate, **⌃⇧Space** with post-processing, **⌃⇧⌘Space** with context, **⌃⇧V** for Paste Last Transcript and **⌃⇧L** to switch word lists. Your shortcuts stay as they are; if you work without Accessibility and still use Option ones, change them to Control in the settings.
- **Option shortcuts still end the dictation.** The second press of a dictation shortcut with ⌥ but without ⌃ or ⌘ now ends the dictation through DanyWhere, and the text waits for **⌃⌥⌘V**.
- **Apps that were open before.** An app that was already open when DanyWhere set up its shortcuts only knows them after a restart. In such an app the second press now ends the dictation, puts the text on the clipboard and shows **Copied – Restart the App** with the app's name. Paste with ⌘V and restart the app once.

macOS uses ⌃Space to switch input sources when that shortcut is on (System Settings → Keyboard → Keyboard Shortcuts → Input Sources). If you use it, choose another dictation shortcut in DanyWhere.
