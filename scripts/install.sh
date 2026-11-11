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
# Copie puis renommage : remplacer sur place un fichier que REAPER a chargé peut le faire planter.
cp target/release/libreaper_aidrummer.dylib "$destination/reaper_aidrummer.dylib.new"
mv -f "$destination/reaper_aidrummer.dylib.new" "$destination/reaper_aidrummer.dylib"

echo "Extension installée dans $destination. Redémarre REAPER s'il est ouvert, puis cherche « AI Drummer » dans la liste des actions."
