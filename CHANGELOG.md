# Changelog

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) ; une version par jalon.

## [0.2.0] — 2026-10-31 · J2 Explorateur et onglets

### Ajouté
- Explorateur : arbre du dossier ouvert, chargé dossier par dossier (5 000 entrées listées en ~4 ms), repliable au clavier et à la souris, panneau redimensionnable (`<` / `>`, Ctrl+Alt+←/→), Ctrl+B pour l'afficher, Ctrl+E pour y aller (F-01).
- Icônes et couleurs par extension et nom de fichier (120 entrées, Nerd Fonts), repli ASCII (F-02, NF-12).
- Créer (`n`, `Shift+N`), renommer (F2, `r`), déplacer (`m`), mettre à la corbeille (Suppr, avec confirmation) ; jamais d'écrasement d'une cible existante (F-03).
- `.gitignore` respecté, fichiers cachés affichés avec `.` (F-04).
- Onglets : plusieurs fichiers ouverts, `●` si modifié, Ctrl+W avec avertissement, Ctrl+PageSuiv/PagePréc, Alt+1..9, clic ; vue (défilement, curseur, historique) propre à chaque onglet (F-20, F-21).
- `ide [dossier|fichier]` : dossier racine de l'explorateur.
- Aperçu J3 : coloration tree-sitter incrémentale (Rust, Python, C, JS/TS/TSX, HTML, CSS, JSON, TOML, Markdown), thème sombre Dark+ (thème clair embarqué, pas encore sélectionnable) (F-30, F-31).

### Corrigé
- Une confirmation de sauvegarde ne marque plus propre un autre onglet du même chemin, ni un document renommé pendant l'écriture.
- Le focus explorateur ne laisse plus coller ni couper dans le document caché.
- Renommer un fichier ouvert, ou le dossier qui le contient, met l'onglet à jour ; un fichier mis à la corbeille reste ouvert, marqué modifié, et Ctrl+S le recrée.

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
