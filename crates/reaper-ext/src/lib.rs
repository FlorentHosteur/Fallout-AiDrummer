//! Extension Reaper d'AI Drummer.
//!
//! Elle ajoute des actions à REAPER. Le travail d'analyse est fait par `aidrummer-engine` ;
//! ici on ne fait que lire le projet et afficher le résultat.

mod project;

use std::error::Error;
use std::sync::OnceLock;

use aidrummer_engine::selection_report;
use fragile::Fragile;
use reaper_low::PluginContext;
use reaper_macros::reaper_extension_plugin;
use reaper_medium::{
    CommandId, HookCommand, OwnedGaccelRegister, ProjectContext, Reaper, ReaperSession,
    TimeRangeType,
};

/// Garde la session en vie tant que Reaper tourne. `Fragile` garantit qu'on n'y touche
/// que depuis le fil principal de Reaper.
static SESSION: OnceLock<Fragile<ReaperSession>> = OnceLock::new();
static SUMMARY_COMMAND: OnceLock<CommandId> = OnceLock::new();
static ANALYZE_COMMAND: OnceLock<CommandId> = OnceLock::new();

/// Dossier, dans les ressources de REAPER, où l'extension écrit ses fichiers.
const DATA_DIR: &str = "AIDrummer";
const LAST_ANALYSIS_FILE: &str = "derniere-analyse.json";

#[reaper_extension_plugin]
fn plugin_main(context: PluginContext) -> Result<(), Box<dyn Error>> {
    let mut session = ReaperSession::load(context);
    register_action(
        &mut session,
        &SUMMARY_COMMAND,
        "AIDRUMMER_PROJECT_SUMMARY",
        "AI Drummer : résumé du projet",
    )?;
    register_action(
        &mut session,
        &ANALYZE_COMMAND,
        "AIDRUMMER_ANALYZE_SELECTION",
        "AI Drummer : analyser la sélection",
    )?;
    session.plugin_register_add_hook_command::<Actions>()?;
    let _ = SESSION.set(Fragile::new(session));
    Ok(())
}

fn register_action(
    session: &mut ReaperSession,
    slot: &OnceLock<CommandId>,
    command_name: &'static str,
    description: &'static str,
) -> Result<(), Box<dyn Error>> {
    let command = session.plugin_register_add_command_id(command_name)?;
    let _ = slot.set(command);
    session.plugin_register_add_gaccel(OwnedGaccelRegister::without_key_binding(
        command,
        description,
    ))?;
    Ok(())
}

struct Actions;

impl HookCommand for Actions {
    fn call(command_id: CommandId, _flag: i32) -> bool {
        let Some(session) = SESSION.get() else {
            return false;
        };
        let reaper = session.get().reaper();
        if SUMMARY_COMMAND.get() == Some(&command_id) {
            print_project_summary(reaper);
            true
        } else if ANALYZE_COMMAND.get() == Some(&command_id) {
            print_selection_analysis(reaper);
            true
        } else {
            false
        }
    }
}

fn print_project_summary(reaper: &Reaper) {
    let project = ProjectContext::CurrentProject;
    let tempo = reaper.master_get_tempo().get();
    let tracks = reaper.count_tracks(project);
    let selection = reaper
        .get_set_loop_time_range_2_get(project, TimeRangeType::TimeSelection)
        .map(|range| {
            format!(
                "de {:.3} s à {:.3} s",
                range.start.get(),
                range.end.get()
            )
        })
        .unwrap_or_else(|| "aucune".to_string());
    reaper.show_console_msg(format!(
        "AI Drummer\n  tempo du projet : {tempo:.2} BPM\n  pistes : {tracks}\n  sélection de temps : {selection}\n"
    ));
}

fn print_selection_analysis(reaper: &Reaper) {
    let analysis = match project::analyze_selection(reaper) {
        Ok(analysis) => analysis,
        Err(message) => {
            reaper.show_console_msg(format!("AI Drummer : {message}\n"));
            return;
        }
    };
    let mut text = selection_report(&analysis);
    match save_last_analysis(reaper, &analysis) {
        Ok(path) => text.push_str(&format!("  Détail complet : {path}\n")),
        Err(message) => text.push_str(&format!("  Détail complet non enregistré : {message}\n")),
    }
    reaper.show_console_msg(text);
}

/// Écrit l'analyse complète en JSON dans les ressources de REAPER et renvoie le chemin du fichier.
fn save_last_analysis(
    reaper: &Reaper,
    analysis: &aidrummer_engine::SelectionAnalysis,
) -> Result<String, String> {
    let directory = reaper.get_resource_path(|path| path.join(DATA_DIR));
    std::fs::create_dir_all(&directory).map_err(|e| format!("{directory} : {e}"))?;
    let file = directory.join(LAST_ANALYSIS_FILE);
    let json = serde_json::to_string_pretty(analysis).map_err(|e| e.to_string())?;
    std::fs::write(&file, json).map_err(|e| format!("{file} : {e}"))?;
    Ok(file.to_string())
}
