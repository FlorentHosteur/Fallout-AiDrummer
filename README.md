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
| Extension chargée par REAPER, avec l'action « AI Drummer : résumé du projet » | compile sur Windows et macOS ; pas encore essayée dans REAPER |
| Analyse d'un fichier audio en ligne de commande (tempo, tonalité, temps) | fonctionne ; le tempo est juste sur des clics de synthèse, pas encore essayé sur de la vraie musique |
| Mesure de l'écart du jeu par rapport au clic du projet | fonctionne, testée |
| Lecture des pistes du projet, écriture MIDI, appel à une IA, panneau | à faire |

Limite connue : la grille de temps et la tonalité renvoyées par la bibliothèque d'analyse (`stratum-dsp`) sont peu fiables. Elles seront remplacées par une détection d'attaques calée sur le tempo du projet REAPER.

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
2. Ouvre la liste des actions (*Actions → Show action list*).
3. Cherche « AI Drummer » et lance « AI Drummer : résumé du projet ».

La console de REAPER affiche le tempo du projet, le nombre de pistes et la sélection de temps.

Si l'action n'apparaît pas, l'extension n'a pas été chargée : vérifie que le fichier est bien dans `UserPlugins` et que REAPER a été redémarré.

## Essayer l'analyse sans REAPER

```powershell
cargo run --release -p aidrummer -- analyze chemin\vers\riff.wav --project-bpm 120
```

Le résultat sort en JSON : tempo, tonalité, position des temps, et l'écart du jeu par rapport à une grille à 120 BPM (`grid_fit`). `mean_offset_ms` positif veut dire que le jeu est en retard sur le clic ; `spread_ms` mesure sa régularité. Les formats WAV, FLAC, MP3, OGG et AAC sont lus.

## Lancer les tests

```powershell
cargo test --workspace --release
```

Un test est volontairement ignoré : il documente l'imprécision de la grille de temps de `stratum-dsp`.

## Organisation du code

| Dossier | Rôle |
|---|---|
| `crates/engine` | Le moteur : décodage audio, analyse. Ne dépend pas de REAPER, se teste seul. |
| `crates/cli` | La commande `aidrummer`, pour essayer le moteur sans REAPER. |
| `crates/reaper-ext` | L'extension chargée par REAPER. Elle s'appuie sur [reaper-rs](https://github.com/helgoboss/reaper-rs). |
| `scripts` | Compilation et installation dans REAPER. |
| `docs/pilotage.html` | Source de la page de suivi du projet. |

`reaper-rs` n'est pas publié sur crates.io : il est pris sur son dépôt Git, à la version figée dans `Cargo.lock`.
