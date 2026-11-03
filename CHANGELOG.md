# Changelog

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) ; une version par jalon.

## [Non publié]

### Ajouté
- Go : coloration (`.go`) et serveur de langage par défaut gopls.
- Vue : les fichiers `.vue` sont traités comme du HTML (coloration, serveur HTML, Emmet).

### Corrigé
- Mise en page HTML, CSS et JSON : les serveurs vscode-*-language-server reçoivent `provideFormatter`, sans quoi ils n'annoncent pas la mise en page.

## [1.1.0] — 2026-11-03 · Langages web et confort d'édition

### Ajouté
- Repli des blocs (selon l'indentation) : `Alt+-` replie le bloc du curseur, `Alt+=` le déplie, `Alt+_` / `Alt++` replient / déplient tout ; clic dans la gouttière pour basculer une ligne. La gouttière marque d'un `-` les blocs repliables et d'un `+` les blocs repliés (suivis de `⋯`), les flèches le sautent, il se déplie si le curseur y arrive (recherche, F12, annuler).
- Paires automatiques : taper `(`, `[`, `{`, `"` ou `'` insère la fermante derrière le curseur ; taper la fermante déjà présente la saute, et `Retour arrière` entre une paire vide retire les deux. Guillemets seulement hors d'un mot (`l'arbre`) ; pas de `'` en Rust.
- HTML et PHP : taper le `>` d'une balise ouvrante insère sa fermante (`<div>|</div>`), sauf éléments vides (`<br>`, `<img>`…) et `/>`.
- Parenthèse, crochet ou accolade sous le curseur (ou juste avant) mis en surbrillance avec son vis-à-vis.
- Mise en page à la sauvegarde (`Ctrl+S`) par le serveur de langage ; `format_on_save = false` dans `[editor]` pour la couper. Sans serveur, ou si l'on tape pendant la mise en page, le fichier est sauvegardé tel quel.
- Onglets : `×` (ou `●` si modifié) pour fermer d'un clic.
- `Alt+Shift+F` ou `Ctrl+0` (ce dernier seulement dans les terminaux à protocole clavier étendu) met le fichier en page avec son serveur de langage (rustfmt via rust-analyzer, Prettier intégré aux serveurs web…), en une seule annulation, curseur conservé.
- PHP : coloration (`.php`, `.phtml`), y compris le HTML hors des balises `<?php ?>`, et complétion avec intelephense (PHP et HTML).
- Abréviations Emmet `!` (page HTML5) et `!!!` (doctype), développées par `Tab` dans les fichiers HTML et PHP.
- Emmet complet dans la liste de complétion (HTML, CSS, PHP) avec emmet-language-server, lancé en plus du serveur du langage.
- Complétion : curseur placé au premier arrêt du snippet (`class="|"`, `<li>|</li>`, `foo(|)`), blocs multilignes alignés sur la ligne courante.
- Serveurs de langage par défaut pour HTML, CSS, JSON (vscode-langservers-extracted), PHP (intelephense), JavaScript et TypeScript (typescript-language-server).

### Corrigé
- macOS : après un plantage, un redémarrage dans les 2 minutes ne proposait pas les copies de secours (seul l'âge du verrou comptait, pas l'existence du processus).

### Divers
- Dépôt public sur GitHub, page de projet en anglais (`README.md`, version française dans `README.fr.md`), fichier `LICENSE` (MIT). Première CI macOS au vert.

## [1.0.0] — 2026-10-31 · J5 Version 1.0

Les 21 exigences « Must » du cahier des charges sont implémentées, relues et testées.

### Ajouté
- Ctrl+Début / Ctrl+Fin, Ctrl+← / Ctrl+→ par mot (Shift étend la sélection), Alt+Retour arrière (ou Ctrl+Retour arrière si le terminal le distingue) efface le mot précédent.
- F1 ouvre la palette de commandes ; protocole clavier étendu activé sur kitty, WezTerm, foot, Ghostty, Alacritty.
- Bandeau permanent tant qu'une copie de secours attend une réponse (Alt+B / Alt+N).

### Corrigé
- Copies de secours : une par instance (une autre instance ne peut plus proposer ni supprimer les vôtres), dernière copie écrite sur SIGHUP / SIGTERM (fermeture du terminal), copie immédiate après un renommage, écriture en échec retentée, copies gardées si une sauvegarde échoue à la sortie, modifications faites pendant une proposition toujours copiées, texte produit hors de la boucle, scan au démarrage en tâche de fond.
- Complétion : listes incomplètes redemandées à la frappe ; une réponse périmée ne fait plus ignorer la suivante.
- didSave envoyé même si l'on tape pendant l'écriture ; `[keys]` ne détourne plus une commande lancée depuis la palette.
- Fichiers de plus de 5 Mo non synchronisés avec le serveur de langage : frappe en ~20 ms sur 62 Mo au lieu de ~200 ms (NF-03).

### Vérifié
- Essais en terminal réel (tmux) : explorateur, palettes, sauvegarde, complétion rust-analyzer, F12 ; plantage `kill -9` puis restauration.
- NF-01 premier rendu 6–35 ms ; NF-02 frappe ~2 ms de traitement ; NF-03 fichier de 62 Mo ouvert et éditable ; NF-04 ~20 Mo au repos ; compilation avec Rust 1.88 (vérifiée en CI).

## [0.4.0] — 2026-10-31 · J4 LSP, début de J5

### Ajouté
- Client LSP maison (JSON-RPC sur stdio, délais et annulations) ; serveurs démarrés automatiquement selon le langage : rust-analyzer, pyright, clangd, ou `[lsp.<langage>]` dans la config utilisateur (F-40).
- Complétion à la frappe et par Ctrl+Espace, plage du serveur respectée (F-41).
- Diagnostics soulignés, marqueurs dans la gouttière, compteurs dans la barre de statut, panneau Problèmes (Ctrl+Shift+M), F8 / Shift+F8 (F-42).
- F12 aller à la définition, Ctrl+K infobulle (F-43).
- Serveur lent, absent ou planté : l'édition continue, redémarrage automatique (3 fois), diagnostics périmés effacés (NF-05).
- Palettes : Ctrl+P fichiers du projet (recherche floue nucleo), Ctrl+Shift+P ou F1 commandes (F-50, F-51).
- Branche git dans la barre de statut (F-70).
- `[keys]` : raccourcis personnalisés vers les commandes de la palette.
- Copie de secours des fichiers modifiés toutes les 30 s, proposée au redémarrage après un plantage (Alt+B restaurer, Alt+N ignorer) (NF-07).
- Session : onglets, curseurs et défilement rouverts au démarrage sans argument (F-80).

### Sécurité
- La config d'un projet (`.ide/config.toml`) ne peut pas choisir de commande à exécuter : `[lsp.*]` y est refusé.

## [0.3.0] — 2026-10-31 · J3 Coloration

### Ajouté
- Coloration syntaxique tree-sitter incrémentale : Rust, Python, C, JavaScript, TypeScript/TSX, HTML, CSS, JSON, TOML, Markdown ; une frappe + recoloration sur 10 000 lignes en ~1,7 ms ; désactivée au-delà de 5 Mo (F-30, NF-03).
- Thèmes sombre (Dark+) et clair (Light+) embarqués, contrastes vérifiés (WCAG) (F-31).
- Configuration `~/.config/ide/config.toml`, surchargée par `<projet>/.ide/config.toml` : `editor.theme`, `editor.tab_size`, `editor.insert_spaces`, `editor.icons` (`nerd` | `ascii`), `explorer.width`, `explorer.show_hidden` ; erreurs affichées sans empêcher le démarrage.
- Rafraîchissement automatique de l'explorateur sur changement disque (F-05).
- Fichier modifié par un autre programme : rechargé s'il n'a pas de modifications, sinon Alt+R recharge et Alt+K garde votre version ; rien n'est jamais écrasé sans choix explicite (NF-09).

### Performance
- Premier rendu en ~1,5 ms ; la surveillance du disque démarre en fond (NF-01).

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
