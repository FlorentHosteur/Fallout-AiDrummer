# Compile l'extension et la copie dans le dossier UserPlugins de REAPER (Windows).
# Messages sans accents : Windows PowerShell 5.1 lit mal l'UTF-8 sans BOM.
param(
    [string]$ResourcePath = (Join-Path $env:APPDATA 'REAPER')
)

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)

if (-not (Test-Path $ResourcePath)) {
    throw "Dossier de ressources REAPER introuvable : $ResourcePath. Passe le bon chemin avec -ResourcePath (dans REAPER : Options > Show REAPER resource path)."
}

cargo build --release -p reaper_aidrummer
if ($LASTEXITCODE -ne 0) {
    throw 'La compilation a echoue.'
}

$destination = Join-Path $ResourcePath 'UserPlugins'
New-Item -ItemType Directory -Force $destination | Out-Null
try {
    Copy-Item 'target\release\reaper_aidrummer.dll' $destination -Force
} catch {
    throw "Copie impossible vers $destination. Ferme REAPER puis relance le script. ($_)"
}

Write-Host "Extension installee dans $destination. Demarre REAPER et cherche 'AI Drummer' dans la liste des actions."
