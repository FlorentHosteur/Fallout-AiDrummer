//! `aidrummer` : le moteur en ligne de commande, pour essayer l'analyse sans Reaper.

use std::path::PathBuf;
use std::process::ExitCode;

use aidrummer_engine::{accent_map, analyze, decode_mono, detect_onsets, grid_fit, Grid};

const USAGE: &str = "\
Usage :
  aidrummer accents <fichier audio> --project-bpm <tempo> [options]
      Range les attaques de l'extrait sur la grille du projet : accents par mesure,
      motif dominant, niveau par mesure, écart du jeu par rapport au clic.

      --beats-per-bar <n>   longueur d'une mesure en noires (4 par défaut ; 3,5 pour du 7/8)
      --slots-per-beat <n>  cases par noire (4 par défaut = doubles croches ; 3 = triolets)
      --grid-origin <s>     position d'un début de mesure dans le fichier, en secondes (0 par défaut)

  aidrummer analyze <fichier audio> [--project-bpm <tempo>] [--grid-origin <s>]
      Cherche le tempo et la tonalité dans l'audio, sans rien savoir du projet.

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

#[derive(Default)]
struct Options {
    file: Option<PathBuf>,
    project_bpm: Option<f64>,
    beats_per_bar: Option<f64>,
    slots_per_beat: Option<f64>,
    grid_origin: Option<f64>,
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();
    let command = args.next().ok_or_else(|| USAGE.to_string())?;
    if command != "accents" && command != "analyze" {
        return Err(format!("commande inconnue : {command}\n\n{USAGE}"));
    }

    let mut options = Options::default();
    while let Some(arg) = args.next() {
        let target = match arg.as_str() {
            "--project-bpm" => &mut options.project_bpm,
            "--beats-per-bar" => &mut options.beats_per_bar,
            "--slots-per-beat" => &mut options.slots_per_beat,
            "--grid-origin" => &mut options.grid_origin,
            _ if arg.starts_with("--") => {
                return Err(format!("option inconnue : {arg}\n\n{USAGE}"))
            }
            _ if options.file.is_none() => {
                options.file = Some(PathBuf::from(arg));
                continue;
            }
            _ => return Err(format!("argument en trop : {arg}\n\n{USAGE}")),
        };
        *target = Some(number(&arg, args.next())?);
    }
    let file = options.file.clone().ok_or_else(|| USAGE.to_string())?;
    let audio = decode_mono(&file).map_err(|e| format!("{} : {e}", file.display()))?;
    let grid_origin = options.grid_origin.unwrap_or(0.0);

    let report = if command == "accents" {
        let bpm = positive("--project-bpm", options.project_bpm)?
            .ok_or("accents a besoin du tempo du projet : --project-bpm <tempo>")?;
        let grid = Grid {
            bpm,
            beats_per_bar: positive("--beats-per-bar", options.beats_per_bar)?.unwrap_or(4.0),
            slots_per_beat: positive("--slots-per-beat", options.slots_per_beat)?
                .unwrap_or(4.0)
                .round() as u32,
            origin_s: grid_origin,
        };
        let onsets = detect_onsets(&audio);
        let map = accent_map(&audio, &onsets, grid);
        serde_json::json!({ "file": file, "duration_s": audio.duration_s(), "accents": map })
    } else {
        let analysis = analyze(&audio).map_err(|e| format!("{} : {e}", file.display()))?;
        let beats: Vec<f64> = analysis.beats.iter().map(|&b| b as f64).collect();
        let fit = positive("--project-bpm", options.project_bpm)?
            .and_then(|bpm| grid_fit(&beats, bpm, grid_origin));
        serde_json::json!({ "file": file, "analysis": analysis, "grid_fit": fit })
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn number(option: &str, value: Option<String>) -> Result<f64, String> {
    let value = value.ok_or_else(|| format!("{option} attend une valeur"))?;
    value
        .replace(',', ".")
        .parse()
        .map_err(|_| format!("{option} : « {value} » n'est pas un nombre"))
}

fn positive(option: &str, value: Option<f64>) -> Result<Option<f64>, String> {
    match value {
        Some(v) if v <= 0.0 || !v.is_finite() => {
            Err(format!("{option} doit être un nombre supérieur à zéro"))
        }
        other => Ok(other),
    }
}
