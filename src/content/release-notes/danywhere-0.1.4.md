## Post-processing in the History

The History now shows what post-processing made of your dictations, and you can post-process entries after the fact.

- **The post-processed text is shown.** An entry that was post-processed shows the result; **Show original** switches to the raw transcript. The copy button copies what is shown.
- **Post-process later.** The wand button (**Post-process**) post-processes an entry's transcript with the prompt selected now. It needs no recording, so it also works for entries whose recording was deleted to stay within the storage limit.
- **Re-transcribe and post-process in one go.** Shift+click on **Re-transcribe** transcribes the recording again and post-processes the result.

Post-processing is switched on under **Advanced → Transcription → Post Processing**; provider, model and prompt are chosen on the **Post Process** page.

## All models to choose from

The **Post Process** page now loads the provider's models by itself, so every model your API key can use is offered right away, not only the one selected. Before, the list had to be loaded with **Refresh models** after every start. That button still reloads it.

## Insert dictations without Accessibility, and keep your clipboard

On a Mac without the Accessibility permission (**Continue without Accessibility**), DanyWhere no longer puts a dictation on the clipboard, so what you copied stays there. The dictation waits until you insert it with **⌃⌥⌘V** in the app you're typing in. This works through a macOS service, "Insert Last Dictation", and needs no permission.

- The small indicator at the edge of the screen says **Ready – ⌃⌥⌘V to insert**, and a third sound, higher than the start sound, plays once the dictation is ready. The **Audio Feedback** switch turns it off together with the start and stop sounds.
- Change the shortcut under **General → Insert Last Dictation**. Apps that are already open pick up a new shortcut after a restart; the same goes for ⌃⌥⌘V right after you continue without Accessibility.
- The service works in apps that support macOS services, such as iTerm2, VS Code, Safari and Chrome. Apple's Terminal doesn't pass the text on: there, **Paste Last Transcript** puts the dictation on the clipboard for you to paste with ⌘V, and the sound plays then too.
