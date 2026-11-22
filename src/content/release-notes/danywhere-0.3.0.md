## Defaults, and profiles that change only what they need

The settings now have three areas in the sidebar: **Defaults**, **Profile Settings** and **Application Settings**. Every setting that concerns a dictation, from the speech model, languages and post-processing to the shortcuts, microphone, sounds, overlay and paste method, has a default, and a profile changes only what it needs. Settings of the app itself, such as the interface language, autostart, updates and storage, are application settings.

- Your profile "Default" became the **Defaults**. Every other profile keeps as its own only what differs from them. Applications assigned to "Default" lost that assignment.
- In a profile, each setting says **Default** or **Own value**, with **Reset** next to an own value; the chain icon holds a value or couples it again. When you change a value a profile shares with the defaults, **Change the Default Instead** and **Undo** are offered for a few seconds.
- Under each setting of the defaults, the profiles that differ show their own value; **Open in …** jumps to it. A profile's page **Profile** lists all its own values and resets the whole profile.
- **In Use**, at the top of the sidebar, chooses what dictations use: the defaults or a profile.

## Prompts: one library, several at once

- The prompts are one library for all profiles. On **Post Process**, check **Active** for every prompt a profile uses and drag them into order. All active prompts go to the model in one request and apply in turn; where two contradict, the later one wins.
- Your prompts were merged: the same text became one prompt, and different texts under the same name became "name · profile". Every profile still has the prompt it used switched on.

## History: new versions on top

- Post-processing, your own edits and transcribing again always add a version on top and delete nothing.
- Drag versions in the track, or use the version's menu: **Move to the Top**, **Move Up**, **Move Down**, **Delete the Current Version**. The top version is the entry's result.
- ✨ post-processes the version you see with the profile's active prompts; the arrow next to it, **Post-process with**, uses any single prompt of the library.
- An entry names the speech model that transcribed it: move the pointer over its date.

## Start and end shortcuts

- A recording starts with a start shortcut and ends with an end shortcut: **General → End Shortcuts**. Each end shortcut has its own key combination and says what is appended, whether the text is post-processed, and whether **Send Context** sends your earlier dictations from the same window along. Add one, for example ⌃↩ that appends Enter.
- Out of the box, the combinations of your two start shortcuts are end shortcuts too: the first ends without post-processing, the second with it, whichever started the recording. So pressing them works as before.
- **When Held** sets how releasing a held start shortcut ends the recording.
- The **Send Context** switch, ⌃⇧X (Alt+Shift+X) and its tray item are gone, and **Append** moved from **Advanced → Output** into each end shortcut. Your settings were carried over.
- Shortcuts are profile values too. Only those of the profile that applies are taken; the others reach the app in front.
- In a combination with a main key, such as ⌃Space, the left and the right ⌃, ⌥, ⇧ and ⌘ now count alike. Combinations show as the system writes them (⌃⌥D; Ctrl+Shift+V elsewhere).
- On Linux, the end shortcuts' own combinations aren't taken.

## Word lists

- Handy's fuzzy correction is gone, together with the **Word Correction Threshold** on Debug. It replaced words that sounded like a list entry and turned correct words into wrong ones, such as "Bild" into "Build".
- Only speech models that take a vocabulary use the word lists while they recognize speech; of those offered, the Whisper models. **With Canary or Parakeet, the word lists now reach only the post-processing.**
- Post-processing gets the word lists as known terms, with every model. It can then write a misheard name as listed and leave a word that fits the sentence.
- The **Word Lists** group names the speech model of its page and says whether it uses the lists; the model list shows **Word lists** as a capability, with a filter.
- The global word list is now the defaults' **Word List**; a profile's own list is **Additional Words**.

## Also new

- Reset buttons say what they do: "Reset … to the factory setting".
- The dictation settings that were on Debug, such as language detection, the sound theme and the AirPods mode, are under **Advanced → Experimental** of the defaults while debug mode is on.
