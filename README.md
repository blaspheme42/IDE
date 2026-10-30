# IDE

IDE de terminal en Rust, inspiré de VSCode : explorateur, éditeur, coloration tree-sitter, LSP et terminal intégré, le tout dans votre terminal.

**État :** J1 · Éditeur (ouvrir, éditer, annuler, sélection, presse-papiers, indentation, rechercher/remplacer)

## Prérequis

- Rust 1.88 ou plus récent (via [rustup](https://rustup.rs))
- Une [Nerd Font](https://www.nerdfonts.com) dans le terminal (recommandée, pour les icônes)

## Commandes

```sh
cargo run -p ide-app      # lance l'IDE (binaire `ide`)
cargo test --workspace    # lance tous les tests
```

## Architecture

| Crate | Rôle | Dépend de |
| --- | --- | --- |
| `ide-app` | Binaire : arguments, boucle d'événements, raccourcis, presse-papiers | `ide-core`, `ide-ui`, `ide-workspace` (toutes à terme) |
| `ide-ui` | Widgets ratatui : explorateur, éditeur, onglets, popups, barre de statut | `ide-core` |
| `ide-core` | Buffers, curseurs, historique, sélection, commandes ; zéro I/O | — |
| `ide-syntax` | Coloration et requêtes tree-sitter, thèmes | `ide-core` |
| `ide-lsp` | Client LSP : lancement des serveurs, JSON-RPC sur stdio, conversion des positions | `ide-core` |
| `ide-workspace` | Arbre de fichiers, watcher, git, icônes, configuration, session | `ide-core` |

Le détail est dans le [cahier des charges](CAHIER_DES_CHARGES.md).

## Utilisation

- `ide <fichier>` : ouvrir un fichier (créé à la première sauvegarde s'il n'existe pas).
- `Ctrl+S` : sauvegarder.
- Flèches, `Début`/`Fin`, `Page préc.`/`Page suiv.` : se déplacer ; avec `Shift`, sélectionner. `Ctrl+A` : tout sélectionner.
- Souris : clic pour placer le curseur, glisser ou `Shift+clic` pour sélectionner, molette pour défiler. La sélection native du terminal reste possible avec `Shift`+glisser (selon le terminal).
- `Ctrl+Z` / `Ctrl+Y` (ou `Ctrl+Shift+Z`) : annuler / rétablir, sans limite.
- `Ctrl+C` / `Ctrl+X` / `Ctrl+V` : copier / couper / coller (sans sélection : la ligne entière). Presse-papiers système, ou interne si aucun n'est disponible (SSH).
- `Tab` / `Shift+Tab` : indenter / désindenter (la sélection entière si elle couvre plusieurs lignes) ; `Entrée` garde l'indentation. Tabulations ou espaces et largeur selon `.editorconfig` (4 espaces par défaut).
- `Ctrl+F` / `Ctrl+H` : rechercher / remplacer. Dans la barre : `Entrée` ou `F3` suivant, `Shift+F3` précédent, `Tab` change de champ, `Entrée` dans « Remplacer » remplace l'occurrence courante, `Alt+Entrée` remplace tout, `Alt+R` regex (`$1`, `${nom}` dans le remplacement), `Alt+C` casse, `Échap` ferme.
- `Ctrl+Q` : quitter ; s'il y a des modifications non sauvegardées, `Ctrl+Q` deux fois pour quitter sans sauvegarder.
- Logs : `$XDG_STATE_HOME/ide/ide.log` (par défaut `~/.local/state/ide/ide.log`).
- `ide --panic-test` : panique volontairement après le premier rendu, pour vérifier que le terminal est bien restauré.
