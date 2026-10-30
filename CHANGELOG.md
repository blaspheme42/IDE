# Changelog

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) ; une version par jalon.

## [0.1.0] — 2026-10-31 · J1 Éditeur

### Ajouté
- Ouvrir, éditer, sauvegarder un fichier ; UTF-8 strict, LF et CRLF conservés, fichiers mixtes octet pour octet (F-10).
- Sauvegarde atomique en tâche de fond, sérialisée et terminée avant de quitter (NF-06).
- Annuler / rétablir illimité avec regroupement des frappes (F-11).
- Sélection clavier (Shift) et souris (clic, glisser, Shift+clic), molette ; copier / couper / coller via le presse-papiers système, repli interne sans serveur graphique (F-12).
- Numéros de ligne, ligne courante surlignée, colonne de diagnostics réservée (F-13).
- Indentation automatique (Entrée, accolades), Tab / Shift+Tab sur sélection, retour arrière au taquet, style lu dans `.editorconfig` (F-14).
- Rechercher / remplacer (Ctrl+F / Ctrl+H), littéral ou regex avec `$1` / `${nom}`, casse, tout remplacer en un seul undo (F-15).

### Fondations (J0)
- Workspace Cargo de 6 crates, boucle d'événements façon Elm (tokio + crossterm), rendu ratatui.
- Terminal restauré après un panic (NF-08), logs dans un fichier, CI Linux et macOS.
