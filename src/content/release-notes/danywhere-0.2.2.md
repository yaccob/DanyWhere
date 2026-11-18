## Post-processing on this computer

- Next to the post-processing provider, the settings now say where it runs: **On this Mac** (**On this computer** on Windows and Linux) or **Cloud**. A provider counts as on this computer when its address is `localhost` or a loopback address, as with Ollama or LM Studio through the **Custom** provider; everything else, a server in your network included, counts as cloud.
- With a model served on this computer, DanyWhere has it loaded as soon as you start a dictation with post-processing. A local server unloads a model after a few idle minutes, and loading a large one again can take half a minute; that now happens while you speak, not after.

## History from the keyboard

- The pencil next to an entry is gone: click into the text to edit it, as before.
- From the keyboard, Tab reaches the entry and Enter or Space opens it for editing (on macOS, Tab may need **Keyboard navigation** switched on in System Settings → Keyboard). When you finish with the keyboard, the focus goes back to the entry, so you can go on from there.
