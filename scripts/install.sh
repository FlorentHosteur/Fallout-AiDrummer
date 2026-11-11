#!/bin/sh
# Compile l'extension et la copie dans le dossier UserPlugins de REAPER (macOS).
# Usage : ./scripts/install.sh [dossier de ressources REAPER]
set -eu

cd "$(dirname "$0")/.."
resource_path="${1:-$HOME/Library/Application Support/REAPER}"

if [ ! -d "$resource_path" ]; then
    echo "Dossier de ressources REAPER introuvable : $resource_path" >&2
    echo "Passe le bon chemin en argument (dans REAPER : Options > Show REAPER resource path)." >&2
    exit 1
fi

cargo build --release -p reaper_aidrummer

destination="$resource_path/UserPlugins"
mkdir -p "$destination"
# REAPER ne charge pas un fichier dont le nom commence par « lib ».
cp target/release/libreaper_aidrummer.dylib "$destination/reaper_aidrummer.dylib"

echo "Extension installée dans $destination. Démarre REAPER et cherche « AI Drummer » dans la liste des actions."
