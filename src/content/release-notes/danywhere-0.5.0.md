## Inserting without the clipboard on a Mac

On macOS, DanyWhere now has an input method of its own. Once you add its input source, dictations go straight into the text field and the clipboard stays as it is, also without Accessibility: there, end shortcuts now insert the text right away instead of leaving it for **Paste Last Transcript**.

- To add it, click **Add in System Settings** under **Application Settings › Accessibility**. In **Keyboard**, click **Edit…** next to **Input Sources**, then **+** and **English**, choose **DanyWhere** and click **Add**.
- DanyWhere selects the input source only while a dictation runs, and typing keeps your keyboard layout. **Paste Last Transcript** inserts the same way.
- With Accessibility the input method is optional; once its source is added, the key presses of **Append**, such as Enter, follow the text.
- Without Accessibility, DanyWhere still can't press keys: an end shortcut's key press, such as Enter to send a chat message, isn't sent, so press it yourself. If you already use DanyWhere without Accessibility, it shows you the setup once at its next start.
- Where the input method can't insert, for example without a text field, DanyWhere pastes with ⌘V, or without Accessibility the text waits as before. To stop, remove **DanyWhere** from **Input Sources** in System Settings.
- If ⌃Space now switches the input source instead of starting a dictation, turn off **Select the previous input source** under System Settings › Keyboard › Keyboard Shortcuts › Input Sources.

## Word lists with Canary and Parakeet

Speech models without a vocabulary, such as Canary and Parakeet, don't use the word lists themselves, so without post-processing a list did nothing. DanyWhere now looks for words that resemble a list entry and, if you switch on **Check Names with the LLM, Also Without Post-Processing**, asks your post-processing provider whether the dictation means that entry. Only what it confirms is replaced.

- The switch is in the **Word Lists** group under **Advanced** while the speech model takes no vocabulary. It's off until you switch it on and needs a post-processing provider with a model; only dictations with such a word go to the provider.
- The group's line says what the lists do with the model in use and offers **Switch to Whisper Large v3 Turbo**, which uses them while it recognizes speech but transcribes noticeably slower, and, without a provider, **Set Up Post-Processing**.
- In a dictation with post-processing, the words found go along as a hint, whether the switch is on or not.

## Also new

- **General** puts the shortcuts in order: first **Start Shortcuts** with **Shortcut Behavior**, then **End Shortcuts**, opening with **When Held** and closing with **Cancel Shortcut**, then **Other Shortcuts**.
- A short press is now called tapping: **Shortcut Behavior** offers **Auto**, **Hold** and **Tap** (before: Toggle), and **When Tapped** heads the list of end shortcuts.
- The overlay's pills are as wide as their text, so the word list and "On the clipboard" are no longer cut off. Without Accessibility, a dictation that waits says "Ready – ⌃⇧V to insert" instead of "On the clipboard – ⌘V to paste".
