## Nachbearbeitung im Verlauf

Der Verlauf zeigt jetzt, was die Nachbearbeitung aus deinen Diktaten gemacht hat, und du kannst Einträge nachträglich nachbearbeiten.

- **Der nachbearbeitete Text wird angezeigt.** Ein nachbearbeiteter Eintrag zeigt das Ergebnis; mit **Original anzeigen** wechselst du zum Rohtext. Der Kopieren-Knopf kopiert, was gerade angezeigt wird.
- **Nachträglich nachbearbeiten.** Der Zauberstab-Knopf (**Nachbearbeiten**) bearbeitet den Text eines Eintrags mit dem gerade ausgewählten Prompt nach. Er braucht keine Aufnahme und funktioniert deshalb auch bei Einträgen, deren Aufnahme gelöscht wurde, um im Speicherlimit zu bleiben.
- **Neu transkribieren und nachbearbeiten in einem Schritt.** Umschalt+Klick auf **Erneut transkribieren** transkribiert die Aufnahme noch einmal und bearbeitet das Ergebnis nach.

Die Nachbearbeitung schaltest du unter **Erweitert → Transkription → Nachbearbeitung** ein; Anbieter, Modell und Prompt wählst du auf der Seite **Nachbearbeitung**.

## Alle Modelle zur Auswahl

Die Seite **Nachbearbeitung** lädt die Modelle des Anbieters jetzt selbst. Zur Auswahl stehen damit gleich alle Modelle, die dein API-Schlüssel nutzen kann, nicht nur das eingestellte. Bisher musstest du die Liste nach jedem Start mit **Modelle aktualisieren** laden; der Knopf lädt sie weiterhin neu.

## Diktate ohne Bedienungshilfen einfügen, die Zwischenablage bleibt unberührt

Auf einem Mac ohne die Berechtigung Bedienungshilfen (**Ohne Bedienungshilfen fortfahren**) legt DanyWhere ein Diktat nicht mehr in die Zwischenablage; was du kopiert hast, bleibt dort. Das Diktat wartet, bis du es in der App, in der du schreibst, mit **⌃⌥⌘V** einfügst. Das geht über einen macOS-Dienst, „Letztes Diktat einfügen“, und braucht keine Berechtigung.

- Die kleine Anzeige am Bildschirmrand sagt **Bereit – ⌃⌥⌘V zum Einfügen**, und ein dritter Ton, höher als der Startton, kommt, sobald das Diktat bereitliegt. Der Schalter **Audio-Feedback** schaltet ihn zusammen mit Start- und Endton ab.
- Das Kürzel änderst du unter **Allgemein → Letztes Diktat einfügen**. Apps, die schon geöffnet sind, übernehmen ein neues Kürzel nach einem Neustart; das gilt auch für ⌃⌥⌘V direkt nach dem Fortfahren ohne Bedienungshilfen.
- Der Dienst funktioniert in Apps, die macOS-Dienste unterstützen, etwa iTerm2, VS Code, Safari und Chrome. Apples Terminal gibt den Text nicht weiter: Dort legt **Letzte Transkription einfügen** das Diktat in die Zwischenablage, damit du es mit ⌘V einfügst, und auch dann kommt der Ton.
