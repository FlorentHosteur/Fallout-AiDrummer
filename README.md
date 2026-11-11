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
| Extension chargée par REAPER, avec onze actions | fonctionne dans REAPER 7 sous macOS ; sous Windows, compile mais n'a pas encore été chargée |
| « Analyser la sélection » : carte d'accents de chaque piste audio, et lecture de la batterie MIDI existante | fonctionne ; exact sur un projet de synthèse, plausible mais non vérifié à l'oreille sur un vrai projet |
| « Écrire une ébauche de batterie » : item MIDI calé sur les accents du morceau, par règles | fonctionne |
| « Batterie avec l'IA » : une IA choisit le jeu (pulsation, charleston, densité, roulement), avec ou sans consigne écrite | fonctionne contre un faux fournisseur local ; pas encore essayé avec un vrai fournisseur |
| La carte d'accents sur un fichier audio, en ligne de commande | fonctionne |
| Recherche du tempo dans l'audio, sans le projet | fonctionne ; juste sur des clics de synthèse |
| Panneau ancrable au style terminal (boutons, consigne, réglages de l'IA, conseils, journal) | s'ouvre avec ReaImGui 0.10 sous macOS ; ses boutons n'ont pas tous été essayés à la main |
| Conseils sur une batterie existante, avec copie corrigée ; avis de l'IA | fonctionne ; conseils tirés de règles simples, à juger à l'oreille |
| Composition à partir de zéro, sans audio dans la sélection | fonctionne ; essayée sur un projet vide en 3/4 |

Limite connue : la grille de temps et la tonalité renvoyées par la bibliothèque `stratum-dsp` (commande `analyze`) sont peu fiables. La carte d'accents ne s'en sert pas : elle part du tempo du projet REAPER.

## Installer sans compiler

La page [Publications](https://github.com/FlorentHosteur/Fallout-AiDrummer/releases/latest) du dépôt propose l'extension déjà compilée.

### Windows

1. Télécharge `reaper_aidrummer.dll`.
2. Ferme REAPER.
3. Dans l'explorateur de fichiers, tape `%APPDATA%\REAPER\UserPlugins` dans la barre d'adresse et dépose-y le fichier. Si REAPER est installé en mode portable, le dossier `UserPlugins` est dans son dossier d'installation.
4. Relance REAPER : *Actions → Show action list*, tape « AI Drummer ».

### macOS (Apple Silicon)

1. Télécharge `reaper_aidrummer-arm64.dylib`.
2. Ferme REAPER et dépose le fichier dans `~/Library/Application Support/REAPER/UserPlugins`.
3. Relance REAPER. Si macOS refuse de charger le fichier, autorise-le dans *Réglages Système → Confidentialité et sécurité*.

### Pour le panneau

Le panneau demande en plus ReaPack et ReaImGui : voir « Le panneau » plus bas. Les actions fonctionnent sans eux.

Pour mettre à jour, remplace le fichier par celui de la publication suivante.

## Installer le kit de développement

Cette partie ne sert que pour compiler soi-même ou modifier le code.

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

Deux façons de lancer AI Drummer : la liste des actions de REAPER (*Actions → Show action list*, puis taper « AI Drummer »), ou le panneau.

### Le panneau

Le panneau regroupe tout dans une fenêtre ancrable : les boutons ANALYSER, ÉBAUCHE et BATTERIE AVEC L'IA, le choix du style (boutons `<` et `>`), la consigne pour l'IA, ses réglages (adresse, modèle, clé, liste des modèles), les conseils sur la batterie existante avec leurs cases à cocher, et un journal qui remplace la console.

Il a besoin de l'extension ReaImGui, version 0.10 ou plus récente :

1. Installe [ReaPack](https://reapack.com) (un fichier à déposer dans `UserPlugins`, expliqué sur son site), puis redémarre REAPER.
2. Dans REAPER : *Extensions → ReaPack → Browse packages*, cherche « ReaImGui », clic droit → *Install*, puis *Apply*.
3. Redémarre REAPER.

Ouvre ensuite le panneau avec l'action « AI Drummer : ouvrir ou fermer le panneau » (tu peux lui donner un raccourci ou un bouton de barre d'outils). Sans ReaImGui, cette action rappelle ces étapes et les autres actions restent utilisables.

Tous les messages sont aussi gardés dans `AIDrummer/journal.txt`, dans le dossier de ressources de REAPER.

### Les actions, une par une

1. Démarre REAPER et ouvre un projet.
2. Fais une sélection de temps sur le passage à étudier (quelques mesures).
3. Ouvre la liste des actions (*Actions → Show action list*), cherche « AI Drummer » et lance « AI Drummer : analyser la sélection ».

La console de REAPER affiche, pour chaque piste qui sonne dans la sélection, son motif rythmique dominant et sa mise en place :

```
AI Drummer : analyse de la sélection
  mesures 89 à 96 (3:12.000 à 3:29.455) · 110.00 BPM · mesure de 4 noires · 8.0 mesures
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

- Les repères sont ceux de la règle de REAPER : numéros de mesure du projet, et temps en minutes:secondes.
- Le tempo et les mesures viennent du projet, pas de l'audio. Si le tempo change dans la sélection, la console le signale et la grille utilisée est une moyenne.
- Le son lu est celui des items, avant les effets de la piste. Les items muets ne comptent pas : leur piste apparaît dans « Sans son ».
- Une piste est lue comme batterie si son nom, l'un de ses effets, ou un dossier qui la contient parle de batterie (« drums », « Batterie », « Superior Drummer », « Drum Kit »…).
- Les notes sont rangées par élément d'après la table Toontrack (EZdrummer 3, Superior Drummer 3) complétée par General MIDI. Les articulations de charleston de cette table n'ont pas été vérifiées dans Superior Drummer ; une note inconnue est listée à part plutôt que devinée.
- L'analyse ne modifie jamais le projet.

### Les styles

Un style impose une façon de jouer. Il fait deux choses :

- **sans IA**, l'ébauche applique le jeu de départ du style au lieu des choix par défaut ;
- **avec l'IA**, la pré-consigne du style (ses règles de jeu, son jeu de départ et des exemples « situation → choix ») est ajoutée aux instructions envoyées. L'IA part du jeu de départ et ne retouche que ce que le passage ou ta consigne demandent ; un choix qu'elle ne donne pas garde la valeur du style. L'avis de l'IA sur une batterie existante est lui aussi demandé pour ce style.

Styles fournis :

| Style | Jeu de départ |
|---|---|
| Rock | Caisse claire sur 2 et 4, charleston à la croche, roulement de fin. |
| Punk | Frappe au maximum, grosse caisse dense, sans nuances. |
| Funk | Charleston à la double croche, notes fantômes, grosse caisse syncopée, frappe modérée. |
| Metal | Grosse caisse collée aux riffs, roulement toutes les 4 mesures. |
| Death Metal | Double pédale en continu, charleston ouvert, frappe au maximum ; blast beat sur les passages les plus intenses. |
| Indus | Grosse caisse sur chaque temps, charleston à la double croche, motif mécanique sans roulement. |
| Prog | Ride, notes fantômes, grosse caisse qui suit les accents décalés, roulement toutes les 4 mesures. |
| Doom / Stoner | Demi-temps, charleston ouvert, peu de notes frappées fort. |

**Choisir un style.** Dans le panneau, section STYLE, avec les boutons `<` et `>`. Sans le panneau, l'action « AI Drummer : style de batterie suivant » passe au style suivant et l'affiche (après le dernier : aucun style). Le choix est gardé d'une session à l'autre dans `AIDrummer/style.txt`.

**Écrire ton propre style.** Le dossier `AIDrummer/styles`, dans le dossier de ressources de REAPER, contient un modèle, `_modele.json` (le style Death Metal). Copie-le sous un autre nom sans le « _ » de tête, par exemple `black-metal.json`, et modifie-le :

| Champ | Sens |
|---|---|
| `id` | Identifiant court. S'il reprend celui d'un style fourni (`rock`, `punk`, `funk`, `metal`, `death-metal`, `indus`, `prog`, `doom`), ton fichier le remplace. |
| `name`, `description` | Le nom et la phrase affichés. |
| `prompt` | La pré-consigne : les règles de jeu du style, en phrases, telles qu'on les dirait à un batteur. |
| `plan` | Le jeu de départ. Mêmes champs que la réponse de l'IA : `feel` (`normal`, `half_time`, `double_time`, `blast`), `hat` (`eighths`, `sixteenths`, `quarters`, `open`, `ride`, `none`), `kick` (`accents`, `four_on_floor`, `double_bass`), `kick_density` (0 à 1), `ghost_notes`, `crash`, `fill`, `fill_every` (roulement toutes les N mesures, 0 = jamais), `intensity` (0 à 1). |
| `examples` | Des exemples à suivre : une `situation` en une phrase et le `plan` qui lui va (seuls les champs qui changent). |

Les fichiers sont relus à chaque action ; un fichier illisible est signalé dans le journal et laissé de côté. Les styles fournis sont dans [crates/engine/styles](crates/engine/styles).

### Écrire une ébauche de batterie

Avec une sélection de temps, lance « AI Drummer : écrire une ébauche de batterie sur la sélection ». L'extension crée une piste « AI Drummer » (si elle n'existe pas) et y pose un item MIDI sur la sélection :

- grosse caisse sur le premier temps et sur chaque accent fort du morceau ;
- caisse claire sur les temps 2 et 4 ;
- charleston fermé à la croche ;
- crash au début.

Les notes écrites (36, 38, 42, 49) sont communes à General MIDI et à la table Toontrack. La piste n'a pas d'instrument : pose ta batterie virtuelle dessus, ou glisse l'item sur ta piste de batterie. Une seule annulation (*Édition → Annuler*) retire tout.

Avec un style choisi, ces choix par défaut sont remplacés par le jeu de départ du style (double pédale, charleston ouvert et roulements réguliers en Death Metal, par exemple). Les roulements ajoutent les notes 45 et 43 (toms).

C'est un point de départ à retoucher, fait de règles simples. Pour un jeu choisi selon le morceau, voir « Batterie avec l'IA ».

### Batterie avec l'IA

L'IA ne reçoit pas l'audio et n'écrit pas les notes. Elle reçoit le compte rendu chiffré du passage (celui de l'analyse, plus le niveau sonore par mesure) et choisit comment jouer : pulsation normale, demi-temps, tempo doublé ou blast beat ; charleston, charleston ouvert ou ride ; grosse caisse sur les accents, sur chaque temps ou en double pédale, et sa densité ; notes fantômes ; crash ; roulement de fin ou toutes les N mesures ; intensité. Si un style est choisi, elle part du jeu de départ de ce style (voir « Les styles »). Le moteur écrit ensuite les notes, grosse caisse calée sur les accents du morceau.

**Réglages.** Lance « AI Drummer : réglages de l'IA ». La console donne le chemin du fichier `AIDrummer/reglages-ia.json` (créé au premier appel) dans le dossier de ressources de REAPER. Ouvre-le avec un éditeur de texte et complète-le :

```json
{
  "base_url": "https://openrouter.ai/api/v1",
  "model": "",
  "api_key": ""
}
```

| Réglage | Sens |
|---|---|
| `base_url` | L'adresse d'une API compatible OpenAI. `https://openrouter.ai/api/v1` pour OpenRouter, `https://api.openai.com/v1` pour OpenAI, ou toute autre adresse qui parle le même protocole (un serveur local, par exemple). |
| `model` | Le nom du modèle chez ce fournisseur. « AI Drummer : lister les modèles de l'IA » affiche les noms disponibles ; copie celui que tu veux. Exemples chez OpenRouter, relevés le 11 novembre 2026 : `anthropic/claude-sonnet-5.5`, `anthropic/claude-haiku-5.5`, `openai/gpt-6.1-terra`, `openai/gpt-6-luna`, `mistralai/mistral-large-4-0`. |
| `api_key` | Ta clé d'accès. Elle est enregistrée en clair dans ce fichier : ne le partage pas. Tu peux la laisser vide et la donner par la variable d'environnement `AIDRUMMER_API_KEY`. |

Le fichier est relu à chaque demande : pas besoin de redémarrer REAPER.

**Utilisation.** Avec une sélection de temps :

- « AI Drummer : batterie avec l'IA sur la sélection » laisse l'IA choisir seule ;
- « AI Drummer : batterie avec l'IA, avec une consigne » ouvre une boîte de saisie. Écris ce que tu veux (« plus lourd, en demi-temps », « léger, à la ride, sans crash ») ; la consigne passe avant le goût de l'IA.

REAPER reste utilisable pendant l'attente. À la réponse, la console affiche les choix de l'IA et son explication, et la batterie est écrite sur la piste « AI Drummer ». Une seule annulation la retire. La dernière réponse est gardée dans `AIDrummer/derniere-reponse-ia.json`.

Chaque demande est facturée par ton fournisseur selon le modèle choisi. Le message envoyé tient en quelques centaines de mots.

### Composer à partir de zéro

Les deux actions d'écriture marchent aussi quand rien ne sonne dans la sélection, par exemple dans un projet vide. Règle le tempo et la mesure du projet, sélectionne quelques mesures, puis lance « écrire une ébauche de batterie » ou « batterie avec l'IA ».

La batterie est alors posée sur un motif de base au lieu des accents du morceau : grosse caisse sur le premier temps, puis sur le temps 3, puis sur les contretemps à mesure que la densité monte. Avec l'IA, la consigne fait tout le travail : « rock binaire simple », « ballade en demi-temps à la ride », « punk rapide, charleston ouvert ».

### Conseils sur une batterie existante

Avec une sélection de temps qui contient de l'audio et une batterie MIDI, lance « AI Drummer : conseils sur la batterie de la sélection ». L'extension compare la batterie aux accents du morceau et propose des retouches :

| Conseil | Ce qu'il fait |
|---|---|
| Appuyer des accents forts par une grosse caisse | Ajoute une grosse caisse là où le morceau accentue et où la batterie ne joue ni grosse caisse ni caisse claire. Avec plusieurs pistes audio, seuls comptent les accents joués par au moins deux d'entre elles. |
| Recaler des grosses caisses sur l'accent voisin | Déplace une grosse caisse posée une double croche à côté d'un accent fort. |
| Alléger des grosses caisses qui ne répondent à rien | Retire les grosses caisses jouées entre les temps, là où aucune piste n'attaque. Ce conseil n'apparaît que si la grosse caisse suit le morceau dans l'ensemble (au moins la moitié de ses coups sur des accents) : une double pédale ou un passage où peu de pistes sonnent ne sont pas des écarts. |
| Donner du relief au charleston ou à la ride | Allège les contretemps quand tous les coups ont la même force. |

Chaque conseil indique où il s'applique, avec le numéro de mesure du projet (« mesure 138, contretemps du temps 2 »).

Ta batterie n'est jamais modifiée. « AI Drummer : écrire la batterie corrigée (tous les conseils) » écrit une copie corrigée sur la piste « AI Drummer » ; coupe ta piste d'origine pour comparer, et annule si ça ne te plaît pas. Dans le panneau, tu coches les conseils à garder avant d'écrire la copie.

Ces conseils viennent de règles simples : ils signalent des écarts, pas des fautes. Sur un passage très dense, le premier peut proposer beaucoup de grosses caisses ; garde ce qui sonne.

« AI Drummer : avis de l'IA sur la batterie de la sélection » envoie le même compte rendu, la batterie et ces remarques à ton modèle, qui répond par cinq conseils au plus, en texte. Dans le panneau, ce que tu écris dans la consigne lui est transmis : tu peux y poser une question (« le refrain est-il trop chargé ? »).

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
| `crates/engine` | Le moteur : décodage audio, détection d'attaques, carte d'accents, lecture, écriture et conseils de batterie, styles (`crates/engine/styles`), appel à l'IA. Ne dépend pas de REAPER, se teste seul. |
| `crates/cli` | La commande `aidrummer`, pour essayer le moteur sans REAPER. |
| `crates/reaper-ext` | L'extension chargée par REAPER : actions, lecture et écriture du projet, panneau. Elle s'appuie sur [reaper-rs](https://github.com/helgoboss/reaper-rs) et, pour le panneau, sur [ReaImGui](https://github.com/cfillion/reaimgui). |
| `scripts` | Compilation et installation dans REAPER. |
| `.github/workflows` | Tests à chaque envoi ; publication des binaires quand une étiquette `v…` est poussée. |
| `docs/pilotage.html` | Source de la page de suivi du projet. |

`reaper-rs` n'est pas publié sur crates.io : il est pris sur son dépôt Git, à la version figée dans `Cargo.lock`.
