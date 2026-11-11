# Fallout AI Drummer

Une extension pour [REAPER](https://www.reaper.fm/) qui aide à écrire des parties de batterie MIDI sur un morceau en cours de composition. Elle est écrite en Rust.

Le but, à terme :

- lire les pistes déjà présentes dans le projet (tempo, mesures, accents, tonalité) pour sentir le morceau ;
- relire une batterie MIDI existante et proposer des améliorations ;
- suivre des consignes écrites pour fabriquer la partie de batterie ;
- dérouler une batterie à partir de zéro.

## Où en est le projet

C'est un squelette. Ce qui existe aujourd'hui :

| Élément | État |
|---|---|
| Extension chargée par REAPER, avec trois actions | fonctionne dans REAPER 7 sous macOS ; sous Windows, compile mais n'a pas encore été chargée |
| « Analyser la sélection » : carte d'accents de chaque piste audio, et lecture de la batterie MIDI existante | fonctionne ; exact sur un projet de synthèse, plausible mais non vérifié à l'oreille sur un vrai projet |
| « Écrire une ébauche de batterie sur la sélection » : item MIDI calé sur les accents du morceau | fonctionne ; règles simples, sans IA |
| La carte d'accents sur un fichier audio, en ligne de commande | fonctionne |
| Recherche du tempo dans l'audio, sans le projet | fonctionne ; juste sur des clics de synthèse |
| Conseils sur une batterie existante, consignes écrites, appel à une IA, panneau | à faire |

Limite connue : la grille de temps et la tonalité renvoyées par la bibliothèque `stratum-dsp` (commande `analyze`) sont peu fiables. La carte d'accents ne s'en sert pas : elle part du tempo du projet REAPER.

## Installer le kit de développement

Il faut trois outils : Git, un compilateur C++ et Rust. REAPER 7 doit être installé.

### Windows

Ouvre PowerShell et lance ces trois commandes, une par une :

```powershell
winget install --id Git.Git -e
```

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

```powershell
winget install --id Rustlang.Rustup -e
```

La deuxième installe les outils de compilation C++ de Microsoft et le SDK Windows (plusieurs gigaoctets). Ils sont obligatoires : les extensions REAPER sous Windows doivent être compilées avec la chaîne MSVC.

Ferme PowerShell puis rouvre-le, pour que `git` et `cargo` soient trouvés. Vérifie :

```powershell
cargo --version
```

```powershell
rustup show active-toolchain
```

La seconde commande doit afficher `stable-x86_64-pc-windows-msvc`. Si elle affiche une chaîne `gnu`, lance `rustup default stable-x86_64-pc-windows-msvc`.

### macOS

```bash
xcode-select --install
```

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Récupérer le code

```powershell
git clone https://github.com/FlorentHosteur/Fallout-AiDrummer.git
```

```powershell
cd Fallout-AiDrummer
```

## Compiler et installer l'extension

Ferme REAPER avant d'installer : sous Windows, il verrouille le fichier de l'extension tant qu'il tourne.

### Windows

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

Le script compile l'extension puis copie `reaper_aidrummer.dll` dans `%APPDATA%\REAPER\UserPlugins`. La première compilation télécharge les dépendances et prend quelques minutes.

Si REAPER est installé en mode portable, donne-lui le dossier de ressources (dans REAPER : *Options → Show REAPER resource path in explorer/finder*) :

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -ResourcePath "D:\REAPER"
```

### macOS

```bash
./scripts/install.sh
```

Le script copie `reaper_aidrummer.dylib` dans `~/Library/Application Support/REAPER/UserPlugins`.

### À la main

```powershell
cargo build --release -p reaper_aidrummer
```

Le fichier produit est `target\release\reaper_aidrummer.dll` sous Windows, `target/release/libreaper_aidrummer.dylib` sous macOS. Copie-le dans le dossier `UserPlugins` de REAPER. Sous macOS, retire le préfixe `lib` du nom, sinon REAPER ne le charge pas.

## Utiliser l'extension dans REAPER

1. Démarre REAPER et ouvre un projet.
2. Fais une sélection de temps sur le passage à étudier (quelques mesures).
3. Ouvre la liste des actions (*Actions → Show action list*), cherche « AI Drummer » et lance « AI Drummer : analyser la sélection ».

La console de REAPER affiche, pour chaque piste qui sonne dans la sélection, son motif rythmique dominant et sa mise en place :

```
AI Drummer : analyse de la sélection
  de 192.000 s à 209.455 s · 110.00 BPM · mesure de 4 noires · 8.0 mesures
  Motif dominant par piste (X accent fort, x accent léger, . rien) :
    Basse    X..x ..X. X.x. X...   retard de 5 ms, régularité 2 ms
    Guitare  X..x ..X. X.x. X...   retard de 30 ms, régularité 2 ms
  Sans son dans la sélection : Clic
```

Si une piste de batterie contient du MIDI dans la sélection, la console ajoute le motif de chaque élément et deux mesures de son accord avec le morceau :

```
  Batterie MIDI « drums » (174 coups ; X dans la plupart des mesures, x dans certaines) :
    grosse caisse     X..X ..x. .... ...X
    caisse claire     .... X... .... X...
    charleston fermé  X.X. X.X. X.X. X.X.
    Accents forts du morceau repris par la grosse caisse ou la caisse claire : 70 %
    Coups de grosse caisse posés sur un accent du morceau : 93 %
```

Le détail complet (accents mesure par mesure, niveau sonore de chaque mesure) est écrit en JSON dans le dossier de ressources de REAPER : `AIDrummer/derniere-analyse.json`.

À savoir :

- Le tempo et les mesures viennent du projet, pas de l'audio. Si le tempo change dans la sélection, la console le signale et la grille utilisée est une moyenne.
- Le son lu est celui des items, avant les effets de la piste. Les items muets ne comptent pas : leur piste apparaît dans « Sans son ».
- Une piste est lue comme batterie si son nom, l'un de ses effets, ou un dossier qui la contient parle de batterie (« drums », « Batterie », « Superior Drummer », « Drum Kit »…).
- Les notes sont rangées par élément d'après la table Toontrack (EZdrummer 3, Superior Drummer 3) complétée par General MIDI. Les articulations de charleston de cette table n'ont pas été vérifiées dans Superior Drummer ; une note inconnue est listée à part plutôt que devinée.
- L'analyse ne modifie jamais le projet.

### Écrire une ébauche de batterie

Avec une sélection de temps, lance « AI Drummer : écrire une ébauche de batterie sur la sélection ». L'extension crée une piste « AI Drummer » (si elle n'existe pas) et y pose un item MIDI sur la sélection :

- grosse caisse sur le premier temps et sur chaque accent fort du morceau ;
- caisse claire sur les temps 2 et 4 ;
- charleston fermé à la croche ;
- crash au début.

Les notes écrites (36, 38, 42, 49) sont communes à General MIDI et à la table Toontrack. La piste n'a pas d'instrument : pose ta batterie virtuelle dessus, ou glisse l'item sur ta piste de batterie. Une seule annulation (*Édition → Annuler*) retire tout.

C'est un point de départ à retoucher, fait de règles simples. Le choix du style et des variations par une IA viendra ensuite.

### Vérifier que l'extension est chargée

L'action « AI Drummer : résumé du projet » affiche le tempo, le nombre de pistes et la sélection de temps.

Si les actions n'apparaissent pas, l'extension n'a pas été chargée : vérifie que le fichier est bien dans `UserPlugins` et que REAPER a été redémarré.

## Essayer l'analyse sans REAPER

La commande `accents` range les attaques d'un extrait sur la grille du projet. Donne-lui le tempo du projet :

```powershell
cargo run --release -p aidrummer -- accents chemin\vers\riff.wav --project-bpm 120
```

Le résultat sort en JSON. Les champs utiles :

| Champ | Sens |
|---|---|
| `pattern` | Le motif dominant, un groupe par noire : `X` accent fort, `x` accent léger, `.` rien. Exemple : `X..x ..X. X.x. X...` |
| `folded` | Le même motif en chiffres, une valeur entre 0 et 1 par double croche |
| `bars` | Le détail mesure par mesure, avec le niveau sonore de chaque mesure (`energy`) |
| `timing.mean_offset_ms` | Décalage moyen du jeu par rapport au clic ; positif = en retard |
| `timing.spread_ms` | Régularité du jeu ; petit = régulier |

Options : `--beats-per-bar 3` pour du 3/4 (ou `3,5` pour du 7/8), `--slots-per-beat 3` pour des triolets, `--grid-origin 1,25` si la première mesure commence à 1,25 s dans le fichier.

La commande `analyze` cherche le tempo dans l'audio sans rien savoir du projet :

```powershell
cargo run --release -p aidrummer -- analyze chemin\vers\riff.wav
```

Les formats WAV, FLAC, MP3, OGG et AAC sont lus.

## Lancer les tests

```powershell
cargo test --workspace --release
```

Un test est volontairement ignoré : il documente l'imprécision de la grille de temps de `stratum-dsp`.

## Organisation du code

| Dossier | Rôle |
|---|---|
| `crates/engine` | Le moteur : décodage audio, détection d'attaques, carte d'accents, lecture et ébauche de batterie. Ne dépend pas de REAPER, se teste seul. |
| `crates/cli` | La commande `aidrummer`, pour essayer le moteur sans REAPER. |
| `crates/reaper-ext` | L'extension chargée par REAPER. Elle s'appuie sur [reaper-rs](https://github.com/helgoboss/reaper-rs). |
| `scripts` | Compilation et installation dans REAPER. |
| `docs/pilotage.html` | Source de la page de suivi du projet. |

`reaper-rs` n'est pas publié sur crates.io : il est pris sur son dépôt Git, à la version figée dans `Cargo.lock`.
