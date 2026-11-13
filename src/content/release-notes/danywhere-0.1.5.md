## Earlier dictations as context

A new **Context Hotkey** dictates and then post-processes with the selected prompt plus your earlier dictations from the same window, so names and terms from the conversation or document you're working in come out right. It is **⌥⇧⌘Space** on macOS, **Ctrl+Alt+Shift+Space** on Windows and **Ctrl+Alt+Shift+C** on Linux, and like the post-processing hotkey it works while post-processing is on.

- Set it up on the **Post Process** page under **Context from Earlier Dictations**: how old earlier dictations may be, how many and how many characters go along, and apps whose windows are never used (password managers are preset).
- Earlier dictations go to the post-processing provider together with the current one, including dictations that were never post-processed. The History marks every entry that went out as context.
- The History shows the app and window of each dictation; click it to see only that window's dictations.
- On a Mac without Accessibility, DanyWhere can ask iTerm2, Terminal, Chrome, Safari and the classic Outlook for their window title once you allow it there (**Allow…**). Linux has no window titles yet, so there is no context there.

## Correct the History

The pencil button corrects the text shown, the transcript or the post-processed text. The original stays, and **Undo correction** brings it back. **Delete post-processing** makes the transcript the entry's text again.

After you correct a transcript, the History says that the post-processing came from the earlier version, and the wand post-processes the corrected one. What you corrected is what's copied, pasted again and sent as context.

## One shortcut to start and insert, without Accessibility

On a Mac without the Accessibility permission, press the dictation shortcut again to end the dictation and insert it as soon as it's ready, in apps that support macOS services. This works for all three dictation shortcuts: plain, with post-processing and with context.

- **General → Dictation Shortcut Also Inserts** switches it off, and **When Nothing Is Being Recorded** chooses what the shortcut does when it reaches an app while DanyWhere isn't recording: start a recording, or do nothing.
- **⌃⌥⌘V** now also ends a running dictation and inserts it once it's ready.
- Where nothing can be inserted, such as Apple's Terminal or the Finder, **Paste Last Transcript** ends the recording and puts the text on the clipboard.
- Apps that are already open pick up the shortcuts after a restart. Push-to-talk works as before.
