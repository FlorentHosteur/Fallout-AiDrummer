//! Extension Reaper d'AI Drummer.
//!
//! Première étape : une action qui lit le projet ouvert (tempo, pistes, sélection de temps)
//! et l'écrit dans la console de Reaper. L'analyse audio et l'écriture MIDI viendront s'y brancher.

use std::error::Error;
use std::sync::OnceLock;

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

#[reaper_extension_plugin]
fn plugin_main(context: PluginContext) -> Result<(), Box<dyn Error>> {
    let mut session = ReaperSession::load(context);
    let command = session.plugin_register_add_command_id("AIDRUMMER_PROJECT_SUMMARY")?;
    let _ = SUMMARY_COMMAND.set(command);
    session.plugin_register_add_hook_command::<SummaryCommand>()?;
    session.plugin_register_add_gaccel(OwnedGaccelRegister::without_key_binding(
        command,
        "AI Drummer : résumé du projet",
    ))?;
    let _ = SESSION.set(Fragile::new(session));
    Ok(())
}

struct SummaryCommand;

impl HookCommand for SummaryCommand {
    fn call(command_id: CommandId, _flag: i32) -> bool {
        if SUMMARY_COMMAND.get() != Some(&command_id) {
            return false;
        }
        if let Some(session) = SESSION.get() {
            print_project_summary(session.get().reaper());
        }
        true
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
