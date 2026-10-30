# Cahier des charges — IDE terminal en Rust

30 octobre 2026 · Benjamin Marco · Version en ligne : https://claude.ai/artifact/Q8iKqLdTtSPaBkooC4rG7i

## Contexte et objectifs

Le projet vise un IDE complet qui tourne dans un terminal, écrit en Rust, avec l'ergonomie de VSCode : explorateur de fichiers à gauche, onglets, autocomplétion et diagnostics via LSP, icônes par type de fichier. Il est développé par une seule personne, en projet personnel et vitrine de portfolio.

**Objectifs mesurables**

- Remplacer VSCode pour éditer un projet Python, C ou Rust de taille moyenne (jusqu'à 5 000 fichiers) sans quitter le terminal.
- Démarrage en moins de 100 ms, saisie sans latence perceptible (moins de 16 ms par frame).
- Un binaire unique, installable avec `cargo install`, sans dépendance système hors police Nerd Font.

**Public visé** : développeurs à l'aise au clavier, habitués à VSCode, qui travaillent en SSH ou veulent un outil léger.

**Hors périmètre (v1)** : édition modale façon Vim, système de plugins, débogueur (DAP), édition collaborative, Windows natif (WSL reste supporté).

Question ouverte : nom du projet.

## Périmètre fonctionnel

La v1 couvre les 21 exigences « Must » ci-dessous ; les « Should » et « Could » viennent ensuite, dans l'ordre du planning. Priorités en MoSCoW : M = indispensable, S = important, C = bonus.

| ID | Module | Exigence | Priorité |
| --- | --- | --- | --- |
| F-01 | Explorateur | Arborescence du dossier ouvert, repliable, dans un panneau gauche redimensionnable | M |
| F-02 | Explorateur | Icône et couleur par extension et par nom de fichier (Nerd Fonts) | M |
| F-03 | Explorateur | Créer, renommer, supprimer (vers la corbeille), déplacer un fichier ou dossier | M |
| F-04 | Explorateur | Respect du `.gitignore`, fichiers cachés masquables | M |
| F-05 | Explorateur | Rafraîchissement automatique sur changement disque (watcher) | S |
| F-06 | Explorateur | Couleur des fichiers selon leur statut git (modifié, nouveau, ignoré) | S |
| F-10 | Éditeur | Ouvrir, éditer, sauvegarder ; UTF-8, fins de ligne LF et CRLF conservées | M |
| F-11 | Éditeur | Annuler / rétablir illimité par fichier | M |
| F-12 | Éditeur | Sélection clavier et souris, copier / couper / coller via le presse-papiers système | M |
| F-13 | Éditeur | Numéros de ligne, ligne courante surlignée, gouttière de diagnostics | M |
| F-14 | Éditeur | Indentation automatique, tabulations ou espaces selon `.editorconfig` | M |
| F-15 | Éditeur | Rechercher / remplacer dans le fichier, regex incluses | M |
| F-16 | Éditeur | Multi-curseurs (Ctrl+D) | S |
| F-17 | Éditeur | Repli de code (folding) basé sur tree-sitter | C |
| F-20 | Onglets | Plusieurs fichiers ouverts en onglets, marqueur « non sauvegardé » | M |
| F-21 | Onglets | Avertissement avant de fermer un fichier modifié | M |
| F-22 | Onglets | Écran divisé vertical | S |
| F-30 | Coloration | Coloration syntaxique tree-sitter : Rust, Python, C, JS/TS, HTML, CSS, JSON, TOML, Markdown | M |
| F-31 | Coloration | Thème sombre et thème clair fournis | M |
| F-40 | LSP | Démarrage automatique du serveur de langage selon l'extension | M |
| F-41 | LSP | Autocomplétion en popup, déclenchée à la frappe et par Ctrl+Espace | M |
| F-42 | LSP | Diagnostics (erreurs, warnings) soulignés et listés dans un panneau | M |
| F-43 | LSP | Aller à la définition (F12), infobulle au survol (hover) | M |
| F-44 | LSP | Renommer un symbole (F2), trouver les références | S |
| F-45 | LSP | Formatage du document à la sauvegarde | S |
| F-46 | LSP | Signature help et code actions | C |
| F-50 | Recherche | Palette « ouvrir un fichier » floue (Ctrl+P) | M |
| F-51 | Recherche | Palette de commandes (Ctrl+Shift+P) | M |
| F-52 | Recherche | Recherche texte dans tout le projet (Ctrl+Shift+F) | S |
| F-60 | Terminal | Terminal intégré dans un panneau bas (Ctrl+ù) | S |
| F-70 | Git | Branche courante dans la barre de statut | M |
| F-71 | Git | Marques d'ajout / modification / suppression dans la gouttière | S |
| F-72 | Git | Vue des changements et commit depuis l'IDE | C |
| F-80 | Session | Réouvrir les onglets et la position du curseur au redémarrage | S |

## Exigences non fonctionnelles

Aucune action de l'IDE ne doit pouvoir perdre une modification de l'utilisateur ; la performance vient juste après.

| ID | Catégorie | Exigence | Mesure d'acceptation |
| --- | --- | --- | --- |
| NF-01 | Performance | Démarrage jusqu'à l'affichage | < 100 ms sur un dossier de 5 000 fichiers |
| NF-02 | Performance | Latence de frappe | < 16 ms entre la touche et l'affichage |
| NF-03 | Performance | Gros fichiers | Fichier de 50 Mo ouvert et éditable, coloration désactivée au-delà de 5 Mo |
| NF-04 | Performance | Mémoire | < 100 Mo au repos, hors serveurs LSP |
| NF-05 | Réactivité | Un serveur LSP lent ou planté ne bloque jamais l'interface | LSP tué en cours de session : l'édition continue, un message s'affiche |
| NF-06 | Robustesse | Sauvegarde atomique (fichier temporaire puis renommage) | Coupure pendant la sauvegarde : l'ancien fichier reste intact |
| NF-07 | Robustesse | Copie de secours des fichiers non sauvegardés toutes les 30 s | Crash simulé : les modifications sont proposées au redémarrage |
| NF-08 | Robustesse | Le terminal est toujours restauré, même après un panic | Panic forcé : le shell reste utilisable |
| NF-09 | Robustesse | Fichier modifié sur disque par un autre programme | L'IDE propose de recharger ou de garder sa version |
| NF-10 | Compatibilité | Linux et macOS ; Windows via WSL | CI verte sur les deux OS |
| NF-11 | Compatibilité | Terminaux : Alacritty, Kitty, WezTerm, GNOME Terminal, tmux | Test manuel avant chaque version |
| NF-12 | Compatibilité | Sans Nerd Font, repli sur des icônes ASCII | Option `icons = "ascii"` |
| NF-13 | Accessibilité | Toute action faisable au clavier seul | Aucune fonction réservée à la souris |
| NF-14 | Accessibilité | Contrastes des thèmes fournis | Ratio ≥ 4,5:1 pour le texte (WCAG AA) |
| NF-15 | Sécurité | Aucune exécution de code d'un projet ouvert sans action de l'utilisateur | Les serveurs LSP viennent du `PATH` ou de la config, jamais du projet |

## Architecture technique

Un workspace Cargo de 6 crates, avec une règle : le cœur (`core`) ne dépend ni du terminal ni du réseau, donc il se teste sans écran. Tout ce qui est lent (LSP, disque, git, terminal) tourne dans des tâches `tokio` qui renvoient des messages à la boucle principale ; l'interface n'attend jamais.

**Boucle d'événements** (modèle Elm, comme Helix et Zed) :

1. Un seul canal `mpsc` reçoit tout : touches et souris (crossterm), réponses LSP, événements disque, sortie du terminal intégré, minuteries.
2. `update(state, event)` modifie l'état et peut émettre des commandes (« demande une complétion », « sauvegarde ce fichier »).
3. Les commandes partent vers les tâches de fond ; leurs résultats reviennent par le même canal.
4. `render(state)` redessine l'écran avec ratatui, au plus 60 fois par seconde.

**Crates du workspace**

| Crate | Rôle | Dépend de |
| --- | --- | --- |
| `app` | Binaire : arguments, boucle d'événements, raccourcis | toutes |
| `ui` | Widgets ratatui : explorateur, éditeur, onglets, popups, barre de statut | `core` |
| `core` | Buffers, curseurs, historique, sélection, commandes ; zéro I/O | — |
| `syntax` | Coloration et requêtes tree-sitter, thèmes | `core` |
| `lsp` | Client LSP : lancement des serveurs, JSON-RPC sur stdio, conversion des positions | `core` |
| `workspace` | Arbre de fichiers, watcher, git, icônes, configuration, session | `core` |

**Dépendances retenues**

| Besoin | Crate | Pourquoi |
| --- | --- | --- |
| Rendu terminal | `ratatui` + `crossterm` | Standard de l'écosystème, souris et couleurs 24 bits |
| Texte | `ropey` | Rope : insertion en O(log n), conversions UTF-8 / UTF-16 pour le LSP |
| Coloration | `tree-sitter` + grammaires officielles | Incrémental, mêmes requêtes que Helix et Neovim |
| Asynchrone | `tokio` | Processus LSP, watcher, PTY |
| LSP | `lsp-types` + client JSON-RPC maison (~400 lignes) | Contrôle total des annulations et timeouts |
| Fichiers | `ignore` (parcours + `.gitignore`), `notify` (watcher), `trash` | Briques de ripgrep, fiables |
| Recherche floue | `nucleo` | Le moteur de Helix, très rapide |
| Terminal intégré | `portable-pty` + `vt100` | PTY multiplateforme et émulateur VT |
| Git | `gix` | Pur Rust, pas de libgit2 à compiler |
| Divers | `arboard` (presse-papiers), `serde` + `toml`, `ec4rs` (.editorconfig), `tracing`, `anyhow` | — |

**Schéma** — le cœur ne touche ni l'écran ni le disque, tout le reste dépend de lui :

```
┌──────────────────────────────────────────┐        ┌──────────────────────┐
│ Terminal · clavier, souris, écran        │        │ Hors de l'IDE        │
│ (crossterm)                              │        │ Serveurs LSP (stdio) │
└──────────────────────────────────────────┘        │ Disque, dépôt .git   │
          entrées │      ▲ rendu                    └──────────────────────┘
                  ▼      │                                     ▲ I/O
┌──────────────────────────────────────────┐                   ▼
│ app · boucle d'événements                │  commandes,  ┌────────────────────────┐
│ Un canal mpsc reçoit touches, LSP, disque│◄────────────►│ Tâches de fond tokio   │
│ update(état, événement) puis render(état)│  résultats   │ ┌────────────────────┐ │
└──────────────────────────────────────────┘              │ │ lsp                │ │
          │                      │                        │ │ JSON-RPC sur stdio,│ │
          ▼                      ▼                        │ │ positions UTF-16   │ │
┌──────────────────────┐ ┌──────────────────────┐         │ └────────────────────┘ │
│ ui                   │ │ syntax               │         │ ┌────────────────────┐ │
│ Widgets ratatui :    │ │ tree-sitter          │         │ │ workspace          │ │
│ arbre, éditeur,      │ │ incrémental, thèmes  │         │ │ arbre, watcher,    │ │
│ onglets, popups      │ │                      │         │ │ git, icônes, config│ │
└──────────────────────┘ └──────────────────────┘         │ └────────────────────┘ │
          │                      │                        └────────────────────────┘
          ▼                      ▼                                     ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│ core · zéro I/O, testable sans écran                                            │
│ Buffers (ropey), curseurs, sélection, historique undo/redo, commandes           │
└─────────────────────────────────────────────────────────────────────────────────┘
```

L'interface ne fait qu'envoyer des commandes et recevoir des résultats : un serveur LSP lent ne peut donc pas la geler.

## Interface et raccourcis

La disposition et les raccourcis reprennent ceux de VSCode, pour qu'un utilisateur de VSCode soit productif dès la première minute.

```
┌─ EXPLORATEUR ──────┬─  main.rs ● ─┬─  config.toml ────────────┐
│  src            │  1  use std::io;                              │
│    main.rs      │  2                                            │
│    lib.rs       │  3  fn main() {                               │
│  tests          │  4      let x = io::st│                       │
│  Cargo.toml     │  5  }             ┌─────────────────┐       │
│  README.md      │                │ ƒ stdin()        │       │
│                  │                │ ƒ stdout()       │       │
│                  │                └─────────────────┘       │
│                  ├─ PROBLÈMES ─ TERMINAL ─────────────────────┤
│                  │ ✖ main.rs:4  cannot find value `st`       │
├──────────────────┴───────────────────────────────────────────┤
│  main  ✖ 1 ⚠ 0  │ Rust │ rust-analyzer ✓ │ Ln 4, Col 19 │ UTF-8 │
└──────────────────────────────────────────────────────────────┘
```

Les icônes s'affichent correctement avec une Nerd Font installée dans le terminal.

| Action | Raccourci |
| --- | --- |
| Ouvrir un fichier (palette floue) | Ctrl+P |
| Palette de commandes | Ctrl+Shift+P |
| Sauvegarder / tout sauvegarder | Ctrl+S / Ctrl+Alt+S |
| Fermer l'onglet | Ctrl+W |
| Onglet suivant / précédent | Ctrl+Tab / Ctrl+Shift+Tab |
| Afficher / masquer l'explorateur | Ctrl+B |
| Basculer le focus explorateur ↔ éditeur | Ctrl+E |
| Terminal intégré | Ctrl+ù |
| Rechercher / remplacer | Ctrl+F / Ctrl+H |
| Rechercher dans le projet | Ctrl+Shift+F |
| Annuler / rétablir | Ctrl+Z / Ctrl+Y |
| Autocomplétion | Ctrl+Espace |
| Aller à la définition | F12 |
| Renommer le symbole | F2 |
| Sélectionner l'occurrence suivante | Ctrl+D |
| Commenter la ligne | Ctrl+/ |
| Aller à la ligne | Ctrl+G |
| Quitter | Ctrl+Q |

Certains terminaux interceptent Ctrl+Shift+… ou Ctrl+Tab : chaque raccourci reste redéfinissable (section suivante) et toute action est aussi accessible depuis la palette de commandes.

## Configuration

Toute la configuration tient dans un fichier TOML, `~/.config/<nom>/config.toml`, qu'on peut surcharger par projet avec `.<nom>/config.toml`. L'IDE démarre sans aucun fichier : chaque clé a une valeur par défaut, et une erreur de config s'affiche sans empêcher le démarrage.

```toml
[editor]
theme = "dark"            # dark | light | chemin vers un .toml de thème
tab_size = 4
insert_spaces = true
format_on_save = true
icons = "nerd"            # nerd | ascii

[explorer]
width = 30
show_hidden = false

[keys]
"ctrl+shift+k" = "delete_line"
"ctrl+p" = "open_file_picker"

[lsp.rust]
command = "rust-analyzer"

[lsp.python]
command = "pyright-langserver"
args = ["--stdio"]

[lsp.c]
command = "clangd"

[icons.extensions]
rs = { glyph = "", color = "#dea584" }
```

- **Thèmes** : un thème associe les noms de captures tree-sitter (`keyword`, `function`, `string`…) et les zones de l'interface à des couleurs ; les thèmes sombre et clair sont embarqués dans le binaire.
- **Icônes** : une table par défaut couvre au moins 80 extensions et noms de fichiers (`Cargo.toml`, `Makefile`, `.gitignore`…), surchargeable dans `[icons]`.
- **Raccourcis** : `[keys]` associe une touche à un nom de commande, le même que dans la palette.

## Qualité et tests

Chaque fusion sur `main` passe une CI verte : formatage, lint, tests et build sur Linux et macOS.

| Niveau | Quoi | Outil |
| --- | --- | --- |
| Unitaires | `core` : insertion, suppression, undo/redo, multi-curseurs, conversions de positions UTF-8 ↔ UTF-16 | `cargo test` |
| Propriétés | Suite d'éditions aléatoires puis undo complet = texte d'origine | `proptest` |
| Rendu | Captures de l'interface dans un terminal virtuel comparées à une référence | `ratatui::TestBackend` + `insta` |
| LSP | Faux serveur LSP qui rejoue des réponses enregistrées, plus timeouts et plantages | tests d'intégration |
| Robustesse | Entrées aléatoires sur le buffer et le parseur de config | `cargo-fuzz` |
| Performance | Démarrage, frappe, ouverture d'un fichier de 50 Mo (seuils de NF-01 à NF-03) | `criterion` |

**Conventions**

- `cargo fmt` et `cargo clippy -- -D warnings` obligatoires.
- Aucun `unwrap()` hors tests ; erreurs typées avec `thiserror` dans les bibliothèques, `anyhow` dans `app`.
- Pas de `unsafe` sans commentaire `// SAFETY:` qui le justifie.
- Commits au format Conventional Commits (`feat:`, `fix:`…), une branche par fonctionnalité, version publiée à chaque jalon avec un CHANGELOG.
- Journal de débogage via `tracing` dans un fichier, jamais sur l'écran.

## Planning et risques

La v1.0 est visée en 16 semaines à environ 10 h par semaine. Chaque jalon livre un IDE utilisable, plus complet que le précédent ; un jalon est fini quand son critère passe.

| Jalon | Semaines | Contenu | Validé quand |
| --- | --- | --- | --- |
| J0 · Fondations | 1–2 | Workspace Cargo, boucle d'événements, CI | Fenêtre vide ouverte puis fermée proprement ; terminal restauré après un panic ; CI verte |
| J1 · Éditeur | 3–5 | Rope, curseur, undo, sélection, sauvegarde | Ouvrir, éditer, sauver un fichier (F-10 à F-15) ; tests unitaires et proptest de `core` verts |
| J2 · Explorateur et onglets | 6–7 | Arbre, icônes, `.gitignore`, onglets | Projet de 5 000 fichiers affiché en moins de 100 ms ; F-01 à F-21 |
| J3 · Coloration | 8–9 | tree-sitter, 9 langages, thèmes | 9 langages colorés, thèmes sombre et clair ; frappe sous 16 ms sur 10 000 lignes |
| J4 · LSP | 10–13 | JSON-RPC, complétion, diagnostics, F12 | Complétion avec rust-analyzer, pyright, clangd ; serveur tué : l'édition continue (NF-05) |
| **J5 · Version 1.0** | 14–16 | Palettes, branche git, session, finitions | **Les 21 exigences M validées** ; version 1.0 publiée et utilisée au quotidien |
| J6 · Après la v1 | en continu | Terminal, écran divisé, recherche projet | Exigences S puis C, une par version ; priorités revues après un mois d'usage |

Le LSP (J4) prend le plus de temps : c'est là qu'un retard est le plus probable, donc la marge est prévue dans J5.

| Risque | Probabilité | Impact | Parade |
| --- | --- | --- | --- |
| Le client LSP est plus complexe que prévu (positions UTF-16, annulations, différences entre serveurs) | Haute | Haut | Un seul serveur d'abord (rust-analyzer), faux serveur de test, `lsp-types` pour les messages |
| Abandon ou dérive du périmètre | Moyenne | Haut | MoSCoW strict, version utilisable à chaque jalon, utiliser l'IDE soi-même dès J2 |
| Unicode mal géré (emoji, caractères double largeur, accents composés) | Moyenne | Moyen | `unicode-segmentation` et `unicode-width` dès J1, avec tests dédiés |
| Rendu lent sur gros fichiers | Moyenne | Moyen | Ne dessiner que la zone visible, coloration incrémentale, seuil de 5 Mo |
| Raccourcis interceptés par le terminal | Haute | Faible | Remappage dans `[keys]`, palette de commandes, protocole clavier Kitty via crossterm |
| Terminal intégré instable (PTY, séquences VT) | Moyenne | Faible | Reporté après la v1 (priorité S) |
