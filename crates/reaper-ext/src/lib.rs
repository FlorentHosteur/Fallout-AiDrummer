//! Extension Reaper d'AI Drummer.
//!
//! Elle ajoute des actions à REAPER. Le travail d'analyse et d'écriture est fait par
//! `aidrummer-engine` ; ici on lit le projet, on affiche le résultat et on parle à l'IA.

mod panel;
mod project;

use std::error::Error;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Mutex, OnceLock};

use aidrummer_engine::{ask_plan, list_models, selection_report, AiSettings, DrumPlan, Feel, Hat};
use fragile::Fragile;
use project::Passage;
use reaper_low::PluginContext;
use reaper_macros::reaper_extension_plugin;
use reaper_medium::{
    CommandId, HookCommand, OwnedGaccelRegister, ProjectContext, Reaper, ReaperSession,
    TimeRangeType,
};

/// Garde la session en vie tant que Reaper tourne. `Fragile` garantit qu'on n'y touche
/// que depuis le fil principal de Reaper.
static SESSION: OnceLock<Fragile<ReaperSession>> = OnceLock::new();
/// Les actions de l'extension, avec le numéro que REAPER leur a donné.
static ACTIONS: OnceLock<Vec<(CommandId, Action)>> = OnceLock::new();
/// La demande à l'IA en cours, s'il y en a une. Le minuteur y lit la réponse.
static PENDING: Mutex<Option<Receiver<Answer>>> = Mutex::new(None);

/// Dossier, dans les ressources de REAPER, où l'extension écrit ses fichiers.
const DATA_DIR: &str = "AIDrummer";
const LAST_ANALYSIS_FILE: &str = "derniere-analyse.json";
const LAST_PLAN_FILE: &str = "derniere-reponse-ia.json";
const SETTINGS_FILE: &str = "reglages-ia.json";
const JOURNAL_FILE: &str = "journal.txt";
const JOURNAL_MAX_BYTES: u64 = 512 * 1024;
/// Variable d'environnement qui peut porter la clé d'accès à la place du fichier de réglages.
const API_KEY_ENV: &str = "AIDRUMMER_API_KEY";
/// Nombre de modèles affichés au plus par la liste des modèles.
const MODELS_SHOWN: usize = 400;

#[derive(Clone, Copy)]
enum Action {
    Summary,
    Analyze,
    Sketch,
    AiDrums,
    AiDrumsWithInstruction,
    AiModels,
    AiSettings,
    Panel,
}

const ACTION_TABLE: [(Action, &str, &str); 8] = [
    (
        Action::Summary,
        "AIDRUMMER_PROJECT_SUMMARY",
        "AI Drummer : résumé du projet",
    ),
    (
        Action::Analyze,
        "AIDRUMMER_ANALYZE_SELECTION",
        "AI Drummer : analyser la sélection",
    ),
    (
        Action::Sketch,
        "AIDRUMMER_WRITE_SKETCH",
        "AI Drummer : écrire une ébauche de batterie sur la sélection",
    ),
    (
        Action::AiDrums,
        "AIDRUMMER_AI_DRUMS",
        "AI Drummer : batterie avec l'IA sur la sélection",
    ),
    (
        Action::AiDrumsWithInstruction,
        "AIDRUMMER_AI_DRUMS_INSTRUCTION",
        "AI Drummer : batterie avec l'IA, avec une consigne",
    ),
    (
        Action::AiModels,
        "AIDRUMMER_AI_MODELS",
        "AI Drummer : lister les modèles de l'IA",
    ),
    (
        Action::AiSettings,
        "AIDRUMMER_AI_SETTINGS",
        "AI Drummer : réglages de l'IA",
    ),
    (
        Action::Panel,
        "AIDRUMMER_PANEL",
        "AI Drummer : ouvrir ou fermer le panneau",
    ),
];

/// Ce qu'une demande à l'IA rapporte au fil principal.
enum Answer {
    Models(Result<Vec<String>, String>),
    Plan(Result<DrumPlan, String>, Box<Passage>),
}

#[reaper_extension_plugin]
fn plugin_main(context: PluginContext) -> Result<(), Box<dyn Error>> {
    let mut session = ReaperSession::load(context);
    let mut actions = Vec::new();
    for (action, command_name, description) in ACTION_TABLE {
        let command = session.plugin_register_add_command_id(command_name)?;
        session.plugin_register_add_gaccel(OwnedGaccelRegister::without_key_binding(
            command,
            description,
        ))?;
        actions.push((command, action));
    }
    let _ = ACTIONS.set(actions);
    session.plugin_register_add_hook_command::<Actions>()?;
    session.plugin_register_add_timer(tick)?;
    let _ = SESSION.set(Fragile::new(session));
    Ok(())
}

fn reaper() -> Option<&'static Reaper> {
    SESSION.get().map(|session| session.get().reaper())
}

/// Affiche un texte : dans le journal du panneau, et dans la console de REAPER quand le
/// panneau est fermé.
fn emit(reaper: &Reaper, text: &str) {
    panel::log(text);
    append_to_journal(reaper, text);
    if !panel::is_open() {
        reaper.show_console_msg(format!("{}\n", text.trim_end()));
    }
}

/// Garde une trace de chaque message dans `AIDrummer/journal.txt`. Le fichier repart de zéro
/// quand il dépasse `JOURNAL_MAX_BYTES`.
fn append_to_journal(reaper: &Reaper, text: &str) {
    use std::io::Write;
    let Ok(file) = data_file(reaper, JOURNAL_FILE) else {
        return;
    };
    let too_big = std::fs::metadata(&file).is_ok_and(|m| m.len() > JOURNAL_MAX_BYTES);
    let journal = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(!too_big)
        .truncate(too_big)
        .open(&file);
    if let Ok(mut journal) = journal {
        let _ = writeln!(journal, "{}", text.trim_end());
    }
}

fn say(reaper: &Reaper, message: impl AsRef<str>) {
    emit(reaper, &format!("AI Drummer : {}", message.as_ref()));
}

struct Actions;

impl HookCommand for Actions {
    fn call(command_id: CommandId, _flag: i32) -> bool {
        let Some(action) = ACTIONS
            .get()
            .and_then(|actions| actions.iter().find(|(id, _)| *id == command_id))
            .map(|(_, action)| *action)
        else {
            return false;
        };
        let Some(reaper) = reaper() else {
            return false;
        };
        match action {
            Action::Summary => print_project_summary(reaper),
            Action::Analyze => print_selection_analysis(reaper),
            Action::Sketch => {
                let result = project::prepare_passage(reaper).and_then(|passage| {
                    project::write_passage(reaper, &passage, &DrumPlan::default())
                });
                say(reaper, result.unwrap_or_else(|message| message));
            }
            Action::AiDrums => start_ai_drums(reaper, None),
            Action::AiDrumsWithInstruction => {
                // Annuler la boîte de saisie annule la demande.
                if let Some(instruction) = ask_instruction(reaper) {
                    start_ai_drums(reaper, Some(instruction));
                }
            }
            Action::AiModels => start_models(reaper),
            Action::AiSettings => show_settings(reaper),
            Action::Panel => toggle_panel(reaper),
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
        .map(|range| format!("de {:.3} s à {:.3} s", range.start.get(), range.end.get()))
        .unwrap_or_else(|| "aucune".to_string());
    emit(
        reaper,
        &format!(
            "AI Drummer\n  tempo du projet : {tempo:.2} BPM\n  pistes : {tracks}\n  sélection de temps : {selection}"
        ),
    );
}

fn print_selection_analysis(reaper: &Reaper) {
    let analysis = match project::analyze_selection(reaper) {
        Ok(analysis) => analysis,
        Err(message) => return say(reaper, message),
    };
    let mut text = selection_report(&analysis);
    match save_json(reaper, LAST_ANALYSIS_FILE, &analysis) {
        Ok(path) => text.push_str(&format!("  Détail complet : {path}\n")),
        Err(message) => text.push_str(&format!("  Détail complet non enregistré : {message}\n")),
    }
    emit(reaper, &text);
}

/// Chemin d'un fichier de l'extension dans les ressources de REAPER (le dossier est créé).
fn data_file(reaper: &Reaper, name: &str) -> Result<String, String> {
    let directory = reaper.get_resource_path(|path| path.join(DATA_DIR));
    std::fs::create_dir_all(&directory).map_err(|e| format!("{directory} : {e}"))?;
    Ok(directory.join(name).to_string())
}

fn save_json<T: serde::Serialize>(
    reaper: &Reaper,
    name: &str,
    value: &T,
) -> Result<String, String> {
    let file = data_file(reaper, name)?;
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(&file, json).map_err(|e| format!("{file} : {e}"))?;
    Ok(file)
}

/// Lit les réglages de l'IA. Crée le fichier à compléter s'il n'existe pas.
fn load_settings(reaper: &Reaper) -> Result<(AiSettings, String), String> {
    let file = data_file(reaper, SETTINGS_FILE)?;
    if !std::path::Path::new(&file).exists() {
        std::fs::write(&file, AiSettings::template_json()).map_err(|e| format!("{file} : {e}"))?;
    }
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{file} : {e}"))?;
    let settings = AiSettings::from_json(&text, std::env::var(API_KEY_ENV).ok())
        .map_err(|e| format!("{file} : {e}"))?;
    Ok((settings, file))
}

fn show_settings(reaper: &Reaper) {
    match load_settings(reaper) {
        Ok((settings, file)) => say(
            reaper,
            format!(
                "réglages de l'IA\n  fichier : {file}\n  adresse (base_url) : {}\n  modèle (model) : {}\n  clé (api_key) : {}\n  Adresses courantes : {} (OpenRouter), {} (OpenAI).\n  Modifie le fichier avec un éditeur de texte ; il est relu à chaque demande.\n  Pour voir les modèles disponibles : action « AI Drummer : lister les modèles de l'IA ».",
                settings.base_url,
                if settings.model.is_empty() { "à renseigner" } else { &settings.model },
                if settings.api_key.is_empty() { "à renseigner" } else { "renseignée" },
                aidrummer_engine::ai::OPENROUTER_URL,
                aidrummer_engine::ai::OPENAI_URL,
            ),
        ),
        Err(message) => say(reaper, message),
    }
}

fn ask_instruction(reaper: &Reaper) -> Option<String> {
    reaper
        .get_user_inputs(
            "AI Drummer : consigne pour la batterie",
            1,
            "Consigne :,extrawidth=320",
            "",
            1024,
        )
        .map(|text| text.to_str().trim().to_string())
}

/// Réserve la place de demande en cours. Renvoie `false` si une demande attend déjà.
fn begin_request(reaper: &Reaper, receiver: Receiver<Answer>) -> bool {
    let mut pending = PENDING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if pending.is_some() {
        say(
            reaper,
            "une demande à l'IA est déjà en cours. Attends sa réponse.",
        );
        return false;
    }
    *pending = Some(receiver);
    true
}

fn start_ai_drums(reaper: &Reaper, instruction: Option<String>) {
    let (settings, file) = match load_settings(reaper) {
        Ok(loaded) => loaded,
        Err(message) => return say(reaper, message),
    };
    if settings.model.is_empty() {
        return say(
            reaper,
            format!("aucun modèle choisi. Renseigne « model » (et « api_key ») dans {file}"),
        );
    }
    let passage = match project::prepare_passage(reaper) {
        Ok(passage) => passage,
        Err(message) => return say(reaper, message),
    };
    let (sender, receiver) = mpsc::channel();
    if !begin_request(reaper, receiver) {
        return;
    }
    say(
        reaper,
        format!(
            "demande envoyée à {} ({}). REAPER reste utilisable ; la batterie sera écrite à la réponse.",
            settings.model, settings.base_url
        ),
    );
    // L'appel peut durer des dizaines de secondes : il se fait hors du fil principal.
    std::thread::spawn(move || {
        let plan = ask_plan(&settings, &passage.analysis, instruction.as_deref());
        let _ = sender.send(Answer::Plan(plan, Box::new(passage)));
    });
}

fn start_models(reaper: &Reaper) {
    let settings = match load_settings(reaper) {
        Ok((settings, _)) => settings,
        Err(message) => return say(reaper, message),
    };
    let (sender, receiver) = mpsc::channel();
    if !begin_request(reaper, receiver) {
        return;
    }
    say(
        reaper,
        format!("lecture des modèles de {}…", settings.base_url),
    );
    std::thread::spawn(move || {
        let _ = sender.send(Answer::Models(list_models(&settings)));
    });
}

/// Appelé par REAPER plusieurs fois par seconde, sur le fil principal : relève la réponse de
/// l'IA quand elle est arrivée.
extern "C" fn tick() {
    collect_answer();
    run_panel();
}

/// Relève la réponse de l'IA quand elle est arrivée.
fn collect_answer() {
    let answer = {
        let Ok(mut pending) = PENDING.try_lock() else {
            return;
        };
        let received = match pending.as_ref() {
            None => return,
            Some(receiver) => receiver.try_recv(),
        };
        match received {
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                *pending = None;
                None
            }
            Ok(answer) => {
                *pending = None;
                Some(answer)
            }
        }
    };
    let Some(reaper) = reaper() else {
        return;
    };
    match answer {
        None => say(reaper, "la demande à l'IA s'est interrompue sans réponse."),
        Some(Answer::Models(Err(message))) | Some(Answer::Plan(Err(message), _)) => {
            say(reaper, message)
        }
        Some(Answer::Models(Ok(models))) if panel::is_open() => {
            say(
                reaper,
                format!(
                    "{} modèles proposés : choisis-en un dans la liste du panneau.",
                    models.len()
                ),
            );
            panel::set_models(models);
        }
        Some(Answer::Models(Ok(models))) => {
            let shown: Vec<&str> = models
                .iter()
                .take(MODELS_SHOWN)
                .map(String::as_str)
                .collect();
            say(
                reaper,
                format!(
                    "{} modèles proposés{} :\n  {}\n  Copie un de ces noms dans « model » (action « AI Drummer : réglages de l'IA »).",
                    models.len(),
                    if models.len() > shown.len() {
                        format!(" (les {} premiers)", shown.len())
                    } else {
                        String::new()
                    },
                    shown.join("\n  ")
                ),
            );
        }
        Some(Answer::Plan(Ok(plan), passage)) => write_ai_drums(reaper, &plan, &passage),
    }
}

fn is_busy() -> bool {
    PENDING
        .try_lock()
        .map(|pending| pending.is_some())
        .unwrap_or(true)
}

fn toggle_panel(reaper: &Reaper) {
    let settings = match load_settings(reaper) {
        Ok((settings, _)) => settings,
        Err(message) => return say(reaper, message),
    };
    match panel::toggle(reaper, &settings) {
        Err(message) => say(reaper, message),
        Ok(()) if panel::is_open() => say(reaper, "panneau ouvert."),
        Ok(()) => say(reaper, "panneau fermé."),
    }
}

/// Dessine le panneau et exécute ce que l'utilisateur vient d'y demander.
fn run_panel() {
    let (commands, error) = panel::frame(is_busy());
    if commands.is_empty() && error.is_none() {
        return;
    }
    let Some(reaper) = reaper() else {
        return;
    };
    if let Some(error) = error {
        say(
            reaper,
            format!("le panneau s'est fermé sur une erreur de ReaImGui : {error}"),
        );
    }
    for command in commands {
        match command {
            panel::Command::Analyze => print_selection_analysis(reaper),
            panel::Command::Sketch => {
                let result = project::prepare_passage(reaper).and_then(|passage| {
                    project::write_passage(reaper, &passage, &DrumPlan::default())
                });
                say(reaper, result.unwrap_or_else(|message| message));
            }
            panel::Command::AiDrums(instruction) => {
                start_ai_drums(reaper, Some(instruction).filter(|text| !text.is_empty()))
            }
            panel::Command::ListModels => start_models(reaper),
            panel::Command::SaveSettings(settings) => save_settings(reaper, &settings),
        }
    }
}

fn save_settings(reaper: &Reaper, settings: &AiSettings) {
    let saved = save_json(reaper, SETTINGS_FILE, settings);
    say(
        reaper,
        match saved {
            Ok(file) => format!("réglages de l'IA enregistrés dans {file}"),
            Err(message) => format!("réglages non enregistrés : {message}"),
        },
    );
}

fn write_ai_drums(reaper: &Reaper, plan: &DrumPlan, passage: &Passage) {
    let _ = save_json(reaper, LAST_PLAN_FILE, plan);
    let written = project::write_passage(reaper, passage, plan);
    say(
        reaper,
        format!(
            "choix de l'IA\n  style : {}\n  {}\n  pulsation : {} · main droite : {} · densité de grosse caisse : {:.1} · notes fantômes : {} · crash : {} · roulement : {} · intensité : {:.1}\n  {}",
            if plan.style.is_empty() { "non précisé" } else { &plan.style },
            if plan.comment.is_empty() { "(pas de commentaire)" } else { &plan.comment },
            match plan.feel {
                Feel::Normal => "caisse claire sur 2 et 4",
                Feel::HalfTime => "demi-temps",
                Feel::DoubleTime => "tempo doublé",
            },
            match plan.hat {
                Hat::Eighths => "charleston à la croche",
                Hat::Sixteenths => "charleston à la double croche",
                Hat::Quarters => "charleston à la noire",
                Hat::Open => "charleston ouvert",
                Hat::Ride => "ride",
                Hat::None => "aucune",
            },
            plan.kick_density,
            oui_non(plan.ghost_notes),
            oui_non(plan.crash),
            oui_non(plan.fill),
            plan.intensity,
            written.unwrap_or_else(|message| message),
        ),
    );
}

fn oui_non(value: bool) -> &'static str {
    if value {
        "oui"
    } else {
        "non"
    }
}
