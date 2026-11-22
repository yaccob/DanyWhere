## Grundeinstellungen und Profile, die nur ändern, was sie brauchen

Die Einstellungen haben jetzt drei Bereiche in der Seitenleiste: **Grundeinstellungen**, **Profileinstellungen** und **Applikationseinstellungen**. Jede Einstellung, die ein Diktat betrifft, vom Spracherkennungsmodell, den Sprachen und der Nachbearbeitung bis zu den Tastenkombinationen, dem Mikrofon, den Tönen, dem Overlay und der Einfügemethode, hat einen Grundwert, und ein Profil ändert nur, was es braucht. Einstellungen der App selbst, etwa die Sprache der Oberfläche, Autostart, Updates und Speicher, sind Applikationseinstellungen.

- Aus deinem Profil „Standard“ sind die **Grundeinstellungen** geworden. Jedes andere Profil behält als eigene Werte nur, was davon abweicht. Anwendungen, die „Standard“ zugeordnet waren, haben diese Zuordnung verloren.
- In einem Profil steht unter jeder Einstellung **Grundwert** oder **eigener Wert**, neben einem eigenen Wert **Zurücksetzen**; das Kettensymbol hält einen Wert fest oder koppelt ihn wieder. Änderst du einen Wert, den ein Profil mit den Grundeinstellungen teilt, bietet DanyWhere ein paar Sekunden lang **Stattdessen Grundwert ändern** und **Rückgängig** an.
- Unter jeder Grundeinstellung zeigen die abweichenden Profile ihren eigenen Wert; **In … öffnen** springt dorthin. Die Seite **Profil** eines Profils listet alle seine eigenen Werte und setzt das ganze Profil zurück.
- **In Verwendung** oben in der Seitenleiste wählt, womit Diktate arbeiten: mit den Grundeinstellungen oder einem Profil.

## Prompts: eine Bibliothek, mehrere auf einmal

- Die Prompts sind eine Bibliothek für alle Profile. Hake auf **Nachbearbeitung** jeden Prompt, den ein Profil nutzt, als **Aktiv** an und zieh sie in die gewünschte Reihenfolge. Alle aktiven Prompts gehen in einer Anfrage an das Modell und gelten nacheinander; wo sich zwei widersprechen, gewinnt der spätere.
- Deine Prompts wurden zusammengeführt: Derselbe Text wurde ein Prompt, verschiedene Texte unter demselben Namen wurden zu „Name · Profil“. Jedes Profil hat weiter den Prompt eingeschaltet, den es benutzt hat.

## Verlauf: neue Fassungen kommen nach oben

- Nachbearbeiten, deine eigenen Änderungen und erneutes Transkribieren legen immer eine Fassung oben auf und löschen nichts.
- Zieh Fassungen in der Leiste, oder nimm das Menü der Fassung: **Ganz nach oben**, **Nach oben**, **Nach unten**, **Aktuelle Fassung löschen**. Die oberste Fassung ist das Ergebnis des Eintrags.
- ✨ bearbeitet die angezeigte Fassung mit den aktiven Prompts des Profils nach; der Pfeil daneben, **Nachbearbeiten mit**, nimmt einen einzelnen Prompt der Bibliothek.
- Ein Eintrag nennt das Spracherkennungsmodell, das ihn transkribiert hat: Fahr mit dem Zeiger über sein Datum.

## Start- und Endtasten

- Eine Aufnahme beginnt mit einer Starttaste und endet mit einer Endtaste: **Allgemein → Endtasten**. Jede Endtaste hat ihre eigene Tastenkombination und sagt, was angehängt wird, ob der Text nachbearbeitet wird und ob **Kontext senden** deine früheren Diktate aus demselben Fenster mitschickt. Leg zum Beispiel ⌃↩ an, das Enter anhängt.
- Von Haus aus sind die Tastenkombinationen deiner beiden Starttasten auch Endtasten: Die erste beendet ohne Nachbearbeitung, die zweite mit, gleich welche die Aufnahme begonnen hat. Sie funktionieren also wie bisher.
- **Beim Halten** legt fest, wie das Loslassen einer gehaltenen Starttaste die Aufnahme beendet.
- Der Schalter **Kontext senden**, ⌃⇧X (Alt+Umschalt+X) und sein Eintrag im Tray sind weg, und **Anhängen** ist von **Erweitert → Ausgabe** in jede Endtaste gewandert. Deine Einstellungen wurden übernommen.
- Auch die Tastenkombinationen sind Profilwerte. Es gelten nur die des Profils, das gerade gilt; die anderen erreichen die App vorn.
- In einer Tastenkombination mit einer Haupttaste, etwa ⌃Leertaste, zählen jetzt linke und rechte ⌃, ⌥, ⇧ und ⌘ gleich. Tastenkombinationen erscheinen so, wie das System sie schreibt (⌃⌥D; sonst Strg+Umschalt+V).
- Unter Linux werden die eigenen Tastenkombinationen der Endtasten nicht belegt.

## Wortlisten

- Handys unscharfe Korrektur ist weg, zusammen mit der **Wortkorrektur-Schwelle** auf Debug. Sie ersetzte Wörter, die wie ein Listeneintrag klangen, und machte so aus richtigen Wörtern falsche, etwa aus „Bild“ ein „Build“.
- Nur Spracherkennungsmodelle, die ein Vokabular annehmen, nutzen die Wortlisten schon beim Erkennen; von den angebotenen sind das die Whisper-Modelle. **Mit Canary oder Parakeet erreichen die Wortlisten jetzt nur noch die Nachbearbeitung.**
- Die Nachbearbeitung bekommt die Wortlisten als bekannte Begriffe, mit jedem Modell. So kann sie einen falsch gehörten Namen wie in der Liste schreiben und ein Wort stehen lassen, das in den Satz passt.
- Die Gruppe **Wortlisten** nennt das Spracherkennungsmodell ihrer Seite und sagt, ob es die Listen nutzt; die Modellliste zeigt **Wortlisten** als Fähigkeit, mit einem Filter.
- Die globale Wortliste ist jetzt die **Wortliste** der Grundeinstellungen; die eigene Liste eines Profils heißt **Zusätzliche Wörter**.

## Außerdem neu

- Die Knöpfe zum Zurücksetzen sagen, was sie tun: „… auf die Werkseinstellung zurücksetzen“.
- Die Diktat-Einstellungen, die auf Debug standen, etwa die Erkennung der Sprache, das Sound-Thema und der AirPods-Modus, stehen unter **Erweitert → Experimentell** der Grundeinstellungen, solange der Debug-Modus an ist.
