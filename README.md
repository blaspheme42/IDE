# IDE

IDE de terminal en Rust, inspiré de VSCode : explorateur, éditeur, coloration tree-sitter, LSP et terminal intégré, le tout dans votre terminal.

**État :** les 21 exigences « Must » sont implémentées (v0.4.0) · reste : usage quotidien et essais sur les terminaux de NF-11 avant la 1.0

## Prérequis

- Rust 1.88 ou plus récent (via [rustup](https://rustup.rs))
- Une [Nerd Font](https://www.nerdfonts.com) dans le terminal (recommandée, pour les icônes)

## Commandes

```sh
cargo run -p ide-app      # lance l'IDE (binaire `ide`)
cargo test --workspace    # lance tous les tests
```

## Configuration

`~/.config/ide/config.toml` (ou `$XDG_CONFIG_HOME/ide/config.toml`), surchargé par `<projet>/.ide/config.toml`. Toutes les clés sont facultatives ; une erreur s'affiche sans empêcher le démarrage.

```toml
[editor]
theme = "dark"          # dark | light
tab_size = 4            # quand aucun .editorconfig ne s'applique
insert_spaces = true
icons = "nerd"          # nerd | ascii

[explorer]
width = 30
show_hidden = false

[keys]                  # touche = nom de commande (voir la palette Ctrl+Shift+P)
"f5" = "save"
"ctrl+shift+k" = "close_tab"

[lsp.python]            # remplace le serveur par défaut d'un langage
command = "pylsp"
args = []
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

- `ide [dossier|fichier]` : sans argument, l'explorateur s'ouvre sur le dossier courant ; avec un dossier, sur ce dossier ; avec un fichier, l'ouvre (créé à la première sauvegarde s'il n'existe pas) avec pour racine le dossier courant s'il le contient, sinon le dossier du fichier.
- `Ctrl+S` : sauvegarder.
- Flèches, `Début`/`Fin`, `Page préc.`/`Page suiv.` : se déplacer ; avec `Shift`, sélectionner. `Ctrl+A` : tout sélectionner.
- Souris : clic pour placer le curseur, glisser ou `Shift+clic` pour sélectionner, molette pour défiler. La sélection native du terminal reste possible avec `Shift`+glisser (selon le terminal).
- `Ctrl+Z` / `Ctrl+Y` (ou `Ctrl+Shift+Z`) : annuler / rétablir, sans limite.
- `Ctrl+C` / `Ctrl+X` / `Ctrl+V` : copier / couper / coller (sans sélection : la ligne entière). Presse-papiers système, ou interne si aucun n'est disponible (SSH).
- `Tab` / `Shift+Tab` : indenter / désindenter (la sélection entière si elle couvre plusieurs lignes) ; `Entrée` garde l'indentation. Tabulations ou espaces et largeur selon `.editorconfig` (4 espaces par défaut).
- `Ctrl+F` / `Ctrl+H` : rechercher / remplacer. Dans la barre : `Entrée` ou `F3` suivant, `Shift+F3` précédent, `Tab` change de champ, `Entrée` dans « Remplacer » remplace l'occurrence courante, `Alt+Entrée` remplace tout, `Alt+R` regex (`$1`, `${nom}` dans le remplacement), `Alt+C` casse, `Échap` ferme.
- Explorateur : `Ctrl+B` l'affiche ou le masque, `Ctrl+E` y place le focus ; flèches pour naviguer, `Entrée` ouvre, `n` / `Shift+N` nouveau fichier / dossier, `F2` ou `r` renommer, `m` déplacer, `Suppr` corbeille (confirmation), `.` fichiers cachés, `<` / `>` largeur.
- Onglets : `Ctrl+W` fermer (deux fois si modifié), `Ctrl+PageSuiv` / `Ctrl+PagePréc` suivant / précédent, `Alt+1`…`Alt+9`.
- Palettes : `Ctrl+P` ouvrir un fichier du projet (recherche floue), `Ctrl+Shift+P` toutes les commandes.
- Code (serveur de langage, démarré automatiquement : rust-analyzer, pyright, clangd) : complétion à la frappe ou `Ctrl+Espace` (`Entrée`/`Tab` accepte), `Ctrl+K` infobulle, `F12` aller à la définition, `F8` / `Shift+F8` problème suivant / précédent, `Ctrl+Shift+M` panneau Problèmes. Un serveur absent ou planté n'empêche jamais d'éditer.
- Fichier modifié par un autre programme : rechargé automatiquement s'il n'a pas de modifications, sinon `Alt+R` recharge, `Alt+K` garde votre version.
- `Ctrl+Q` : quitter ; s'il y a des modifications non sauvegardées, `Ctrl+Q` deux fois pour quitter sans sauvegarder.
- Logs : `$XDG_STATE_HOME/ide/ide.log` (par défaut `~/.local/state/ide/ide.log`).
- `ide --panic-test` : panique volontairement après le premier rendu, pour vérifier que le terminal est bien restauré.
