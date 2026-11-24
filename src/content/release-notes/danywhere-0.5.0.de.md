## Einfügen ohne Zwischenablage auf dem Mac

Auf macOS hat DanyWhere jetzt eine eigene Eingabemethode. Sobald du ihre Eingabequelle hinzugefügt hast, kommen Diktate direkt ins Textfeld, und die Zwischenablage bleibt, wie sie ist, auch ohne Bedienungshilfen: Dort fügen Endtasten den Text jetzt sofort ein, statt ihn für **Letzte Transkription einfügen** liegen zu lassen.

- Zum Hinzufügen klick unter **Applikationseinstellungen › Bedienungshilfen** auf **In den Systemeinstellungen hinzufügen**. Klick dort unter **Tastatur** neben **Eingabequellen** auf **Bearbeiten …**, dann auf **+** und **Englisch**, wähl **DanyWhere** und klick auf **Hinzufügen**.
- DanyWhere wählt die Eingabequelle nur, während ein Diktat läuft, und beim Tippen bleibt deine Tastaturbelegung. **Letzte Transkription einfügen** fügt genauso ein.
- Mit Bedienungshilfen ist die Eingabemethode ein Angebot; ist ihre Quelle hinzugefügt, folgen die Tastendrücke aus **Anhängen**, etwa Enter, nach dem Text.
- Ohne Bedienungshilfen kann DanyWhere weiterhin keine Tasten drücken: Der Tastendruck einer Endtaste, etwa Enter zum Absenden einer Chat-Nachricht, wird nicht gesendet; drück ihn selbst. Nutzt du DanyWhere schon ohne Bedienungshilfen, zeigt es dir die Einrichtung beim nächsten Start einmal.
- Kann die Eingabemethode nicht einfügen, etwa ohne Textfeld, fügt DanyWhere mit ⌘V ein, oder ohne Bedienungshilfen wartet der Text wie bisher. Zum Beenden entferne **DanyWhere** in den Systemeinstellungen aus den **Eingabequellen**.
- Schaltet ⌃Leertaste jetzt die Eingabequelle um, statt ein Diktat zu starten, schalte unter Systemeinstellungen › Tastatur › Tastaturkurzbefehle › Eingabequellen **Vorherige Eingabequelle auswählen** aus.

## Wortlisten mit Canary und Parakeet

Sprachmodelle ohne Vokabular wie Canary und Parakeet nutzen die Wortlisten nicht selbst, ohne Nachbearbeitung bewirkte eine Liste deshalb nichts. Jetzt sucht DanyWhere nach Wörtern, die einem Listeneintrag ähneln, und fragt, wenn du **Namen mit dem LLM prüfen, auch ohne Nachbearbeitung** einschaltest, deinen Anbieter der Nachbearbeitung, ob das Diktat diesen Eintrag meint. Ersetzt wird nur, was er bestätigt.

- Der Schalter steht in der Gruppe **Wortlisten** unter **Erweitert**, solange das Sprachmodell kein Vokabular nimmt. Er ist aus, bis du ihn einschaltest, und braucht einen Anbieter für die Nachbearbeitung mit einem Modell; an den Anbieter gehen nur Diktate mit einem solchen Wort.
- Die Zeile der Gruppe sagt, was die Listen beim Modell bewirken, und bietet **Zu Whisper Large v3 Turbo wechseln** an, das sie schon beim Erkennen der Sprache nutzt, aber spürbar langsamer transkribiert, und ohne Anbieter **Nachbearbeitung einrichten**.
- In einem Diktat mit Nachbearbeitung gehen die gefundenen Wörter als Hinweis mit, ob der Schalter an ist oder nicht.

## Außerdem neu

- **Allgemein** ordnet die Tastenkombinationen: zuerst **Starttasten** mit **Verhalten der Tastenkombination**, dann **Endtasten**, beginnend mit **Beim Halten** und endend mit **Abbrechen-Tastenkürzel**, dann **Weitere Tastenkombinationen**.
- Ein kurzer Druck heißt jetzt Antippen: **Verhalten der Tastenkombination** bietet **Automatisch**, **Halten** und **Antippen** (vorher „Umschalten“), und **Beim Antippen** steht über der Liste der Endtasten.
- Die Pillen des Overlays sind so breit wie ihr Text, so werden Wortliste und „In der Zwischenablage“ nicht mehr abgeschnitten. Ohne Bedienungshilfen sagt ein wartendes Diktat „Bereit – ⌃⇧V zum Einfügen“ statt „In der Zwischenablage – ⌘V zum Einfügen“.
