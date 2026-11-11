## Post-processing in the History

The History now shows what post-processing made of your dictations, and you can post-process entries after the fact.

- **The post-processed text is shown.** An entry that was post-processed shows the result; **Show original** switches to the raw transcript. The copy button copies what is shown.
- **Post-process later.** The wand button (**Post-process**) post-processes an entry's transcript with the prompt selected now. It needs no recording, so it also works for entries whose recording was deleted to stay within the storage limit.
- **Re-transcribe and post-process in one go.** Shift+click on **Re-transcribe** transcribes the recording again and post-processes the result.

Post-processing is switched on under **Advanced → Transcription → Post Processing**; provider, model and prompt are chosen on the **Post Process** page.

## All models to choose from

The **Post Process** page now loads the provider's models by itself, so every model your API key can use is offered right away, not only the one selected. Before, the list had to be loaded with **Refresh models** after every start. That button still reloads it.

## A sound when the text waits on the clipboard

When DanyWhere cannot paste a dictation itself, for example after **Continue without Accessibility**, it puts the text on the clipboard for you to paste. A third sound, higher than the start sound, now tells you when the text is there. It plays after **Paste Last Transcript** in that case too. The **Audio Feedback** switch turns it off together with the start and stop sounds.
