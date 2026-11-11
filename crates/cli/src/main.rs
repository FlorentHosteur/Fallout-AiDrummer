//! `aidrummer` : le moteur en ligne de commande, pour essayer l'analyse sans Reaper.

use std::path::PathBuf;
use std::process::ExitCode;

use aidrummer_engine::{analyze, decode_mono, grid_fit};

const USAGE: &str = "\
Usage :
  aidrummer analyze <fichier audio> [--project-bpm <tempo>] [--grid-origin <secondes>]

  --project-bpm   tempo du projet Reaper ; ajoute l'écart du jeu par rapport à cette grille
  --grid-origin   position d'un temps de la grille dans le fichier, en secondes (0 par défaut)

Le résultat est écrit en JSON sur la sortie standard.";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("analyze") => {}
        _ => return Err(USAGE.to_string()),
    }
    let mut file: Option<PathBuf> = None;
    let mut project_bpm: Option<f64> = None;
    let mut grid_origin = 0.0f64;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project-bpm" => project_bpm = Some(number(&arg, args.next())?),
            "--grid-origin" => grid_origin = number(&arg, args.next())?,
            _ if arg.starts_with("--") => return Err(format!("option inconnue : {arg}\n\n{USAGE}")),
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => return Err(format!("argument en trop : {arg}\n\n{USAGE}")),
        }
    }
    let file = file.ok_or_else(|| USAGE.to_string())?;

    let audio = decode_mono(&file).map_err(|e| format!("{} : {e}", file.display()))?;
    let analysis = analyze(&audio).map_err(|e| format!("{} : {e}", file.display()))?;
    let fit = project_bpm.and_then(|bpm| grid_fit(&analysis.beats, bpm, grid_origin));

    let report = serde_json::json!({
        "file": file,
        "analysis": analysis,
        "grid_fit": fit,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn number(option: &str, value: Option<String>) -> Result<f64, String> {
    let value = value.ok_or_else(|| format!("{option} attend une valeur"))?;
    value
        .parse()
        .map_err(|_| format!("{option} : « {value} » n'est pas un nombre"))
}
