## Profile

Ein Profil hält, was bestimmt, wie ein Diktat verarbeitet wird: Spracherkennungsmodell, Sprachen, Nachbearbeitung mit eigenen Prompts, Füllwörter, Wortlisten, Kontext und was angehängt wird. Tastenkombinationen, Mikrofon, API-Schlüssel und Oberfläche sind in allen Profilen gleich. Aus deinen Einstellungen ist das erste Profil geworden.

- Das Profil wählst du oben in der Seitenleiste oder im Tray. Die neue Seite **Profile** listet sie, dupliziert, exportiert und importiert sie; ein exportierter API-Schlüssel wird mit einem Passwort verschlüsselt, das du wählst.
- **Profile je Anwendung:** Gib einem Profil die Anwendungen, für die es gedacht ist, dann nimmt ein Diktat in eine davon dieses Profil, samt Spracherkennungsmodell. **Profil nach Anwendung wählen** (⌃⇧A unter macOS, sonst Alt+Umschalt+A) schaltet das aus und wieder ein, ohne die Liste zu ändern. Unter Windows tippst du die Programmnamen ein; unter Linux kann DanyWhere die Anwendung vorn noch nicht erkennen.
- Ab zwei Profilen zeigt das Overlay, welches ein Diktat nimmt.

## Kontext per Schalter

Das Kontext-Tastenkürzel gibt es nicht mehr, ebenso seinen macOS-Dienst und den Kommandozeilenschalter `--toggle-post-process-with-context`. Stattdessen ist **Kontext senden** ein Schalter je Profil: Solange er an ist, schickt jedes Diktat mit Nachbearbeitung deine früheren Diktate aus demselben Fenster mit. Er ist zu Beginn **aus**. Einschalten kannst du ihn im Tray, in der Seitenleiste, unter **Nachbearbeitung → Kontext aus früheren Diktaten** oder mit ⌃⇧X (sonst Alt+Umschalt+X).

## Verlauf: Fassungen und Suche

- Jeder Eintrag ist ein Stapel von Fassungen: unten die Transkription, darüber jede Bearbeitung, deine oder die der Nachbearbeitung, jeweils auf der Fassung, aus der sie entstand. Klick in den Text, um ihn zu bearbeiten; **Speichern** ändert die Fassung, die du siehst, **Als neue Fassung speichern** legt eine dazu. Die Spur unter dem Text listet die Fassungen und geht zu einer früheren zurück. Deine bisherigen Korrekturen sind zu Fassungen geworden.
- **Verlauf durchsuchen** sucht in allen Einträgen und allen ihren Fassungen, wahlweise mit Groß- und Kleinschreibung, ganzen Wörtern und regulären Ausdrücken. Ein zweites Feld filtert nach App, Fenstertitel oder Dateiname.
- Die Texte stehen in Kästen, die scrollen; zieh am unteren Rand, um sie höher oder niedriger zu machen.

## Nachbearbeitung

- DanyWhere sagt dem Modell jetzt selbst, was es bekommt: eine Transkription aus der Spracherkennung, die es bearbeiten und nicht beantworten soll. Außerdem bittet es das Modell, falsch verstandene Wörter zu korrigieren, wo der Zusammenhang klar macht, was gesagt wurde. Ein Prompt sagt jetzt nur noch, was mit dem Text geschehen soll. Ein Prompt, der noch `${output}` enthält, läuft wie bisher, und DanyWhere nennt solche Prompts beim Start.
- **Denkaufwand**, unter dem Modell, legt fest, wie viel das Modell nachdenkt, je Profil und Anbieter.
- Unter **Erweitert → Transkription** stellst du ein, wie lange die Nachbearbeitung und das Laden der Modellliste dauern dürfen.

## Ausgabe

- **Anhängen** (Erweitert → Ausgabe) ersetzt **Leerzeichen anhängen** und **Automatisch absenden**: beliebiger Text, in dem `\s` ein Leerzeichen und `\n` eine neue Zeile ist, danach ein Tastendruck wie Enter. Deine bisherigen Einstellungen wurden übernommen. Tastendrücke brauchen die Bedienungshilfen; ohne sie wird nur der Text angehängt.
- **Letzte Transkription kopieren** (⌃⇧C unter macOS, sonst Alt+Umschalt+C) legt das letzte Diktat in die Zwischenablage. Während eines Diktats beendet es das Diktat und kopiert den Text, sobald er fertig ist.
- Auf einem Mac ohne Bedienungshilfen fügt **⌃⇧V** (Letzte Transkription einfügen) das letzte Diktat jetzt über den macOS-Dienst ein, statt es in die Zwischenablage zu legen; der Dienst hat keine eigene Tastenkombination (⌃⌥⌘V) mehr. Apps, die schon offen waren, lernen das nach einem Neustart. Hat „Letzte Transkription einfügen“ noch ⌥⇧V von vor 0.1.6, stell es auf eine Kombination mit ⌃ oder ⌘ um.

## Außerdem neu

- Ein Ton, sobald ein Diktat ganz fertig ist, Nachbearbeitung eingeschlossen: **Ton, wenn alles fertig ist**, unter Ton.
- Eine `theme.css` neben den Einstellungen ändert die Farben der App, ohne Neustart: **Debug → Design-Datei öffnen** (⌘⇧D unter macOS, sonst Strg+Umschalt+D, öffnet die Debug-Seite).
- Der Schalter **Experimentelle Funktionen** ist weg; seine sechs Einstellungen stehen immer auf der Debug-Seite.
- Auf Grundlage von Handy 0.10.1. Unter Linux fügen neue Installationen mit **Zwischenablage (Umschalt+Einfg)** ein, das auch in Terminals funktioniert. Wenn du **Direkt** nutzt, lohnt sich der Wechsel unter **Erweitert → Einfügemethode**.
