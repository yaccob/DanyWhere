# DanyWhere's input method: names, link and protocol

DanyWhere inserts dictations on macOS through an input method of its own once the user has added its input source in System Settings (Keyboard → Input Sources). The input method is a small Objective-C bundle (InputMethodKit) that ships inside the app and is copied to `~/Library/Input Methods` when the user sets it up. This file is the contract between the input method (this folder) and the app (`src-tauri/src/input_method/`).

## The rule

1. If the DanyWhere input source is added, DanyWhere inserts through it, with or without Accessibility, without the clipboard.
2. Else, with Accessibility, it pastes with ⌘V as before.
3. Else the text waits as before without Accessibility (the "Insert Last Dictation" service, ⌃⇧V).

Only setups without Accessibility are guided to add the source; with Accessibility it is offered in the settings. DanyWhere selects its source only while a dictation runs (and for "Paste Last"), with the keyboard layout that was in use before, and selects the previous source again afterwards. Dictations and "Paste Last" overlap (a recording starts while the previous dictation's text is still being inserted): each holds the source from its start until its text is in, and the previous source comes back only when the last hold ends.

## Names

For an app with bundle id `<app id>` and product name `<name>` (release `com.yaccob.danywhere` / `DanyWhere`, Dev `com.pais.handy.dev` / `DanyWhere Dev`, NoAX `com.pais.handy.noax` / `DanyWhere NoAX`):

| What                                                                                | Value                                                                                       |
| ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Bundle file name                                                                    | `<name> Input.app`                                                                          |
| `CFBundleIdentifier` = `TISInputSourceID`                                           | `<app id>.inputmethod.DanyWhere` (macOS requires `.inputmethod.` in the id)                 |
| `CFBundleName`, `CFBundleDisplayName` (shown in the input menu and System Settings) | `<name>`                                                                                    |
| `InputMethodConnectionName`                                                         | `<input method id>_Connection`                                                              |
| `DanyWhereAppID` (custom Info.plist key)                                            | `<app id>`                                                                                  |
| `CFBundleVersion`, `CFBundleShortVersionString`                                     | the app's version                                                                           |
| `TISIntendedLanguage`                                                               | `en` (the setup tells the user to search for the name)                                      |
| Place in the app                                                                    | `Contents/Helpers/<name> Input.app`, signed before the app is signed                        |
| Installed copy                                                                      | `~/Library/Input Methods/<name> Input.app`                                                  |
| Log of the input method                                                             | `~/Library/Logs/<app id>/input-method.log` (events only, never typed keys or inserted text) |

The app finds its bundled input method as the `*.app` in `Contents/Helpers` whose `DanyWhereAppID` equals the app's own bundle id. A build without one (plain `tauri build`, `cargo test`, Windows, Linux) has no input method: rule 1 never applies.

## Installing and updating

- **Setup** (the user clicks the button): copy the bundled input method to `~/Library/Input Methods/` (into a temporary name, then rename over the old one), remove `com.apple.quarantine` from the copy, `TISRegisterInputSource(<copy>)`, then open System Settings at Keyboard (macOS 13 and later `x-apple.systempreferences:com.apple.Keyboard-Settings.extension`, before that `x-apple.systempreferences:com.apple.preference.keyboard?InputSources`). `TISEnableInputSource` is not called: macOS ignores it (tried 2026-11-23); the user adds the source.
- **Update** (at every app start): if an installed copy exists and its executable or `Info.plist` differ from the bundled one, ask the running input method to quit (`quit`, else terminate the process with its bundle id), replace the copy as above and register it again. Without an installed copy nothing is installed at start.
- **Added** means: `TISCreateInputSourceList({kTISPropertyInputSourceID: <input method id>}, false)` is not empty (only enabled sources are listed).

## The link

- A Unix domain stream socket. The **app listens**, the input method connects.
- Path: `~/Library/Application Support/<app id>/input-method.sock`; if that is longer than 103 bytes, `<_CS_DARWIN_USER_TEMP_DIR><app id>.input-method.sock`. The directory is `0700`, the socket `0600`. The app removes a stale file before binding and binds again if the file disappears while it runs (checked every 60 s).
- The input method connects at launch, on every `activateServer` while not connected, and every 2 s while its source is selected and it is not connected.
- **Both ends check the peer** before reading or writing anything else: the peer's audit token (`getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, …)`) → `SecCodeCopyGuestWithAttributes` with `kSecGuestAttributeAudit` → `SecCodeCheckValidity` against the requirement `identifier "<expected id>" and certificate leaf = H"<SHA-1 of the checking side's own leaf certificate>"`. The app expects the bundled input method's id, the input method expects `DanyWhereAppID`. A side whose own code has no certificate (ad hoc, unsigned) accepts no peer. Release builds are signed with the Developer ID, Dev and NoAX with the self-signed "DanyWhere Local Signing"; app and input method are always signed with the same certificate.
- One connection at a time; a new verified connection replaces the old one. A write that fails or times out closes the connection, so a half-written line never merges with the next message.
- **On every (re)connection** the app sends the current `layout` and `restore` (both `null` when nothing is selected), and the input method sends `hello` and then `active` for the text field it has, if any. `restore` belongs to the connection that sent it: the input method applies it once when that connection breaks and then forgets it.

## Messages

Newline-delimited JSON objects in UTF-8, each with a `"type"`. Unknown types and unknown fields are ignored, so either side can be newer.

Input method → app:

| Message                                                              | When                                                                                           |
| -------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `{"type":"hello","version":"0.3.3","pid":123}`                       | first, right after connecting                                                                  |
| `{"type":"active","client":"com.apple.TextEdit"}`                    | `activateServer`: a text field of that app now has the input method                            |
| `{"type":"inactive","client":"com.apple.TextEdit"}`                  | `deactivateServer`                                                                             |
| `{"type":"inserted","id":7,"ok":true,"client":"com.apple.TextEdit"}` | after `insert` succeeded                                                                       |
| `{"type":"inserted","id":7,"ok":false,"reason":"no_client"}`         | after `insert` failed; `reason` is `no_client` (no text field has the input method) or `error` |
| `{"type":"caught","id":9,"code":36,"mods":0}`                        | stage 2: a key it was told to catch was pressed                                                |

App → input method:

| Message                                                                | What the input method does                                                                                                                                                                                                                                   |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `{"type":"insert","id":7,"text":"…"}`                                  | `[client insertText:text replacementRange:NSMakeRange(NSNotFound, 0)]` into the text field of the last `activateServer`, then answers `inserted`                                                                                                             |
| `{"type":"layout","source":"com.apple.keylayout.German"}`              | while its source is selected, typing uses this keyboard layout (`TISSetInputMethodKeyboardLayoutOverride`), applied now if active and on every `activateServer`; `"source":null` clears it                                                                   |
| `{"type":"restore","source":"com.apple.keylayout.German"}`             | if the link breaks while its own source is the current one, select this source (`TISSelectInputSource`); `null` clears it                                                                                                                                    |
| `{"type":"catch","id":9,"keys":[{"code":36,"mods":0}],"hold_ms":1500}` | stage 2: on the next key-down matching one of `keys` (key code and device-independent modifier flags; no auto-repeat) while it has a client, send `caught` and hold the key: wait up to `hold_ms` for `release`. `{"type":"catch","id":9,"keys":[]}` disarms |
| `{"type":"release","id":9,"text":"…"}`                                 | stage 2: insert `text`, then let the held key through (`handle` returns `NO`). Without `release` in time the key is swallowed (`YES`) and the text comes later with `insert`                                                                                 |
| `{"type":"quit"}`                                                      | exit (the app replaces the bundle)                                                                                                                                                                                                                           |

Outside stage 2's `catch` the input method lets every key through unchanged (`handle` returns `NO`) and counts nothing.

**An `insert` without a text field waits** up to 0.5 s for the next `activateServer` before it answers `no_client`: the app sends it right after selecting the source, and macOS activates the input method for the focused field a moment later. A newer `insert` ends an older waiting one with `no_client`; a broken link drops a waiting one without inserting it.

**`inserted` with `ok:true` means the client took the text.** Some apps (Chromium, Electron) accept `insertText` even without a focused input field and drop the text; the input method can't tell.

**An `insert` the input method doesn't answer in time, or whose link breaks before the answer, may still have arrived.** The app then inserts it no other way and presses no "Append" keys (`InsertFailure::Unanswered`).

## Stage 2 (Enter as an end shortcut, later)

"Insert and send": while a recording runs, the input method catches the end shortcut Enter, holds it until the text is ready, inserts the text and lets Enter through. How long a key may be held before the app in front suffers is measured first (with the user's go); the app side is built after that.

Open before the app side is built (review of 2026-11-23):

- `release` has no answer. A `release` that comes just after `hold_ms` finds the key already swallowed, and the app can't tell whether the text went in; add e.g. `released {id, ok}` or `hold_timeout {id}`.
- An Enter held longer than the key repeat delay repeats after it is let through, sending more Enters.
- The input method caps `hold_ms` at 10 000 ms and compares only Shift, Control, Option and Command for `mods`.
