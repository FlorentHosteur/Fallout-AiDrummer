//! Le panneau d'AI Drummer : une fenêtre ancrable dessinée avec ReaImGui, au style de vieux
//! terminal monochrome.
//!
//! ReaImGui est une autre extension de REAPER (installée par ReaPack). Elle prête ses fonctions
//! aux extensions natives : on les demande par leur nom au démarrage du panneau, puis on
//! redessine la fenêtre à chaque passage du minuteur de REAPER.
//!
//! Tout ce module tourne sur le fil principal de REAPER.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr::null_mut;
use std::sync::Mutex;

use aidrummer_engine::AiSettings;
use reaper_medium::Reaper;

/// Version de l'API de ReaImGui pour laquelle les signatures ci-dessous sont écrites.
const API_VERSION: &CStr = c"0.10";
const WINDOW_TITLE: &CStr = c"AI Drummer";
/// Nombre de lignes gardées dans le journal.
const LOG_LINES: usize = 400;
const TEXT_CAPACITY: usize = 2048;

// Couleurs du terminal, en 0xRRVVBBAA.
const GREEN: u32 = 0x33FF_66FF;
const GREEN_DIM: u32 = 0x1F9A_45FF;
const BLACK: u32 = 0x060A_07FF;
const PANEL: u32 = 0x0A14_0CFF;
const BUTTON: u32 = 0x0F2A_16FF;
const BUTTON_HOVER: u32 = 0x1B4D_2AFF;
const BUTTON_ACTIVE: u32 = 0x2A7A_41FF;

type Ctx = *mut c_void;
type GetApi = unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void;

/// Les fonctions de ReaImGui utilisées par le panneau. Les signatures viennent de
/// `reaper_imgui_functions.h` (ReaImGui 0.10) : un argument facultatif y est un pointeur,
/// nul quand on ne le donne pas.
struct Api {
    last_error: unsafe extern "C" fn() -> *const c_char,
    create_context: unsafe extern "C" fn(*const c_char, *mut c_int) -> Ctx,
    create_font: unsafe extern "C" fn(*const c_char, *mut c_int) -> *mut c_void,
    attach: unsafe extern "C" fn(Ctx, *mut c_void),
    push_font: unsafe extern "C" fn(Ctx, *mut c_void, f64),
    pop_font: unsafe extern "C" fn(Ctx),
    begin: unsafe extern "C" fn(Ctx, *const c_char, *mut bool, *mut c_int) -> bool,
    end: unsafe extern "C" fn(Ctx),
    begin_child: unsafe extern "C" fn(
        Ctx,
        *const c_char,
        *mut f64,
        *mut f64,
        *mut c_int,
        *mut c_int,
    ) -> bool,
    end_child: unsafe extern "C" fn(Ctx),
    text: unsafe extern "C" fn(Ctx, *const c_char),
    text_colored: unsafe extern "C" fn(Ctx, c_int, *const c_char),
    separator_text: unsafe extern "C" fn(Ctx, *const c_char),
    button: unsafe extern "C" fn(Ctx, *const c_char, *mut f64, *mut f64) -> bool,
    same_line: unsafe extern "C" fn(Ctx, *mut f64, *mut f64),
    input_text: unsafe extern "C" fn(
        Ctx,
        *const c_char,
        *mut c_char,
        c_int,
        *mut c_int,
        *mut c_void,
    ) -> bool,
    input_text_multiline: unsafe extern "C" fn(
        Ctx,
        *const c_char,
        *mut c_char,
        c_int,
        *mut f64,
        *mut f64,
        *mut c_int,
        *mut c_void,
    ) -> bool,
    selectable:
        unsafe extern "C" fn(Ctx, *const c_char, *mut bool, *mut c_int, *mut f64, *mut f64) -> bool,
    set_next_item_width: unsafe extern "C" fn(Ctx, f64),
    set_next_window_size: unsafe extern "C" fn(Ctx, f64, f64, *mut c_int),
    set_scroll_here_y: unsafe extern "C" fn(Ctx, *mut f64),
    begin_disabled: unsafe extern "C" fn(Ctx, *mut bool),
    end_disabled: unsafe extern "C" fn(Ctx),
    push_style_color: unsafe extern "C" fn(Ctx, c_int, c_int),
    pop_style_color: unsafe extern "C" fn(Ctx, *mut c_int),
    push_style_var: unsafe extern "C" fn(Ctx, c_int, f64, *mut f64),
    pop_style_var: unsafe extern "C" fn(Ctx, *mut c_int),
    /// Couleurs remplacées par le thème : (élément, couleur).
    theme: Vec<(c_int, u32)>,
    /// Arrondis mis à zéro.
    square_corners: Vec<c_int>,
    docking: c_int,
    first_use: c_int,
    password: c_int,
    child_borders: c_int,
}

impl Api {
    /// Demande à ReaImGui les fonctions dont le panneau a besoin.
    fn load(reaper: &Reaper) -> Result<Api, String> {
        let context = reaper.low().plugin_context();
        let get_api = unsafe { context.GetFunc(c"ImGui__getapi".as_ptr()) };
        let get_error = unsafe { context.GetFunc(c"ImGui__geterr".as_ptr()) };
        if get_api.is_null() || get_error.is_null() {
            return Err(MISSING_REAIMGUI.to_string());
        }
        // Sûr : ce sont les deux points d'entrée que ReaImGui publie pour les extensions
        // natives, avec ces signatures.
        let get_api: GetApi = unsafe { std::mem::transmute(get_api) };
        let last_error: unsafe extern "C" fn() -> *const c_char =
            unsafe { std::mem::transmute(get_error) };

        // `T` est toujours un type de pointeur de fonction : même taille qu'un pointeur.
        unsafe fn function<T>(get_api: GetApi, name: &CStr) -> Result<T, String> {
            let pointer = unsafe { get_api(API_VERSION.as_ptr(), name.as_ptr()) };
            if pointer.is_null() {
                return Err(format!(
                    "ReaImGui ne fournit pas la fonction {} : version trop ancienne ? Mets ReaImGui à jour par ReaPack.",
                    name.to_string_lossy()
                ));
            }
            Ok(unsafe { std::mem::transmute_copy(&pointer) })
        }
        // Les constantes de ReaImGui sont des fonctions sans argument qui renvoient leur valeur.
        let constant = |name: &CStr| -> Result<c_int, String> {
            let read: unsafe extern "C" fn() -> c_int = unsafe { function(get_api, name)? };
            Ok(unsafe { read() })
        };

        unsafe {
            Ok(Api {
                last_error,
                create_context: function(get_api, c"CreateContext")?,
                create_font: function(get_api, c"CreateFont")?,
                attach: function(get_api, c"Attach")?,
                push_font: function(get_api, c"PushFont")?,
                pop_font: function(get_api, c"PopFont")?,
                begin: function(get_api, c"Begin")?,
                end: function(get_api, c"End")?,
                begin_child: function(get_api, c"BeginChild")?,
                end_child: function(get_api, c"EndChild")?,
                text: function(get_api, c"Text")?,
                text_colored: function(get_api, c"TextColored")?,
                separator_text: function(get_api, c"SeparatorText")?,
                button: function(get_api, c"Button")?,
                same_line: function(get_api, c"SameLine")?,
                input_text: function(get_api, c"InputText")?,
                input_text_multiline: function(get_api, c"InputTextMultiline")?,
                selectable: function(get_api, c"Selectable")?,
                set_next_item_width: function(get_api, c"SetNextItemWidth")?,
                set_next_window_size: function(get_api, c"SetNextWindowSize")?,
                set_scroll_here_y: function(get_api, c"SetScrollHereY")?,
                begin_disabled: function(get_api, c"BeginDisabled")?,
                end_disabled: function(get_api, c"EndDisabled")?,
                push_style_color: function(get_api, c"PushStyleColor")?,
                pop_style_color: function(get_api, c"PopStyleColor")?,
                push_style_var: function(get_api, c"PushStyleVar")?,
                pop_style_var: function(get_api, c"PopStyleVar")?,
                theme: vec![
                    (constant(c"Col_Text")?, GREEN),
                    (constant(c"Col_TextDisabled")?, GREEN_DIM),
                    (constant(c"Col_WindowBg")?, BLACK),
                    (constant(c"Col_ChildBg")?, PANEL),
                    (constant(c"Col_PopupBg")?, PANEL),
                    (constant(c"Col_Border")?, GREEN_DIM),
                    (constant(c"Col_Separator")?, GREEN_DIM),
                    (constant(c"Col_FrameBg")?, PANEL),
                    (constant(c"Col_FrameBgHovered")?, BUTTON),
                    (constant(c"Col_Button")?, BUTTON),
                    (constant(c"Col_ButtonHovered")?, BUTTON_HOVER),
                    (constant(c"Col_ButtonActive")?, BUTTON_ACTIVE),
                    (constant(c"Col_Header")?, BUTTON),
                    (constant(c"Col_HeaderHovered")?, BUTTON_HOVER),
                    (constant(c"Col_TitleBg")?, PANEL),
                    (constant(c"Col_TitleBgActive")?, BUTTON),
                    (constant(c"Col_ScrollbarBg")?, BLACK),
                    (constant(c"Col_TextSelectedBg")?, BUTTON_ACTIVE),
                ],
                square_corners: vec![
                    constant(c"StyleVar_FrameRounding")?,
                    constant(c"StyleVar_WindowRounding")?,
                ],
                docking: constant(c"ConfigFlags_DockingEnable")?,
                first_use: constant(c"Cond_FirstUseEver")?,
                password: constant(c"InputTextFlags_Password")?,
                child_borders: constant(c"ChildFlags_Borders")?,
            })
        }
    }

    /// L'erreur laissée par le dernier appel à ReaImGui, s'il y en a une.
    fn take_error(&self) -> Option<String> {
        let message = unsafe { (self.last_error)() };
        (!message.is_null()).then(|| {
            unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned()
        })
    }
}

const MISSING_REAIMGUI: &str = "le panneau a besoin de l'extension ReaImGui, qui n'est pas installée.\n  1. Installe ReaPack (https://reapack.com), puis redémarre REAPER.\n  2. Dans REAPER : Extensions → ReaPack → Browse packages, cherche « ReaImGui », clic droit → Install, puis Apply.\n  3. Redémarre REAPER et relance cette action.\n  En attendant, toutes les actions « AI Drummer » restent utilisables depuis la liste des actions.";

/// Un champ de saisie : un tampon de taille fixe terminé par un zéro, comme ReaImGui l'attend.
struct TextField(Vec<u8>);

impl TextField {
    fn new(text: &str) -> TextField {
        let mut field = TextField(vec![0; TEXT_CAPACITY]);
        field.set(text);
        field
    }

    fn set(&mut self, text: &str) {
        self.0.fill(0);
        let bytes = text.as_bytes();
        let mut length = bytes.len().min(TEXT_CAPACITY - 1);
        // On ne coupe pas au milieu d'un caractère accentué.
        while !text.is_char_boundary(length) {
            length -= 1;
        }
        self.0[..length].copy_from_slice(&bytes[..length]);
    }

    fn text(&self) -> String {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        String::from_utf8_lossy(&self.0[..end]).trim().to_string()
    }

    fn pointer(&mut self) -> *mut c_char {
        self.0.as_mut_ptr() as *mut c_char
    }
}

/// Ce que l'utilisateur vient de demander dans le panneau.
pub enum Command {
    Analyze,
    Sketch,
    /// Batterie avec l'IA ; la consigne est vide si rien n'a été écrit.
    AiDrums(String),
    ListModels,
    SaveSettings(AiSettings),
}

struct Panel {
    api: Api,
    context: Ctx,
    font: *mut c_void,
    instruction: TextField,
    base_url: TextField,
    model: TextField,
    api_key: TextField,
    models: Vec<String>,
    /// Nombre de lignes du journal déjà affichées : sert à suivre les nouvelles lignes.
    log_seen: u64,
}

thread_local! {
    static PANEL_STATE: RefCell<Option<Panel>> = const { RefCell::new(None) };
}

/// Le journal : (nombre total de lignes écrites, dernières lignes).
static LOG: Mutex<(u64, VecDeque<String>)> = Mutex::new((0, VecDeque::new()));

/// Ajoute un message au journal du panneau.
pub fn log(message: &str) {
    let mut log = LOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for line in message.lines() {
        log.0 += 1;
        log.1.push_back(line.to_string());
        if log.1.len() > LOG_LINES {
            log.1.pop_front();
        }
    }
}

pub fn is_open() -> bool {
    PANEL_STATE.with(|state| state.borrow().is_some())
}

/// Ouvre le panneau, ou le ferme s'il est ouvert.
pub fn toggle(reaper: &Reaper, settings: &AiSettings) -> Result<(), String> {
    if is_open() {
        PANEL_STATE.with(|state| *state.borrow_mut() = None);
        return Ok(());
    }
    let api = Api::load(reaper)?;
    let mut flags = api.docking;
    let context = unsafe { (api.create_context)(WINDOW_TITLE.as_ptr(), &mut flags) };
    if let Some(error) = api.take_error() {
        return Err(format!("ReaImGui : {error}"));
    }
    let font = unsafe { (api.create_font)(c"monospace".as_ptr(), null_mut()) };
    unsafe { (api.attach)(context, font) };
    if let Some(error) = api.take_error() {
        return Err(format!("ReaImGui : {error}"));
    }
    let panel = Panel {
        api,
        context,
        font,
        instruction: TextField::new(""),
        base_url: TextField::new(&settings.base_url),
        model: TextField::new(&settings.model),
        api_key: TextField::new(&settings.api_key),
        models: Vec::new(),
        log_seen: 0,
    };
    PANEL_STATE.with(|state| *state.borrow_mut() = Some(panel));
    Ok(())
}

/// Donne au panneau la liste des modèles du fournisseur.
pub fn set_models(models: Vec<String>) {
    PANEL_STATE.with(|state| {
        if let Some(panel) = state.borrow_mut().as_mut() {
            panel.models = models;
        }
    });
}

/// Dessine le panneau s'il est ouvert et renvoie ce que l'utilisateur vient de demander, ainsi
/// que l'erreur de ReaImGui qui l'a fait fermer, le cas échéant.
/// `busy` : une demande à l'IA est en cours.
pub fn frame(busy: bool) -> (Vec<Command>, Option<String>) {
    PANEL_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(panel) = state.as_mut() else {
            return (Vec::new(), None);
        };
        let (commands, keep_open) = panel.draw(busy);
        let error = panel.api.take_error();
        if error.is_some() || !keep_open {
            *state = None;
        }
        (commands, error)
    })
}

fn c_string(text: &str) -> CString {
    CString::new(text.replace('\0', " ")).unwrap_or_default()
}

impl Panel {
    fn draw(&mut self, busy: bool) -> (Vec<Command>, bool) {
        let ctx = self.context;
        let mut commands = Vec::new();
        let mut open = true;

        unsafe {
            for &(element, color) in &self.api.theme {
                (self.api.push_style_color)(ctx, element, color as c_int);
            }
            for &corner in &self.api.square_corners {
                (self.api.push_style_var)(ctx, corner, 0.0, null_mut());
            }
            (self.api.push_font)(ctx, self.font, 13.0);

            let mut first_use = self.api.first_use;
            (self.api.set_next_window_size)(ctx, 560.0, 680.0, &mut first_use);
            // Dans ReaImGui, End ne s'appelle que si Begin a renvoyé vrai.
            if (self.api.begin)(ctx, WINDOW_TITLE.as_ptr(), &mut open, null_mut()) {
                self.draw_content(busy, &mut commands);
                (self.api.end)(ctx);
            }

            (self.api.pop_font)(ctx);
            let mut corners = self.api.square_corners.len() as c_int;
            (self.api.pop_style_var)(ctx, &mut corners);
            let mut colors = self.api.theme.len() as c_int;
            (self.api.pop_style_color)(ctx, &mut colors);
        }
        (commands, open)
    }

    /// Le contenu de la fenêtre. `unsafe` : appelle ReaImGui avec le contexte du panneau.
    unsafe fn draw_content(&mut self, busy: bool, commands: &mut Vec<Command>) {
        let ctx = self.context;
        let api = &self.api;
        let button =
            |label: &CStr| unsafe { (api.button)(ctx, label.as_ptr(), null_mut(), null_mut()) };
        let same_line = || unsafe { (api.same_line)(ctx, null_mut(), null_mut()) };
        let dim =
            |text: &CStr| unsafe { (api.text_colored)(ctx, GREEN_DIM as c_int, text.as_ptr()) };
        let mut fill = -1.0f64;

        unsafe {
            dim(c"AI DRUMMER :: ASSISTANT BATTERIE");

            (api.separator_text)(ctx, c"PASSAGE".as_ptr());
            dim(c"Fais une sélection de temps dans REAPER, puis :");
            if button(c"ANALYSER") {
                commands.push(Command::Analyze);
            }
            same_line();
            if button(c"ÉBAUCHE") {
                commands.push(Command::Sketch);
            }
            same_line();
            let mut disabled = busy;
            (api.begin_disabled)(ctx, &mut disabled);
            if button(c"BATTERIE AVEC L'IA") {
                commands.push(Command::AiDrums(self.instruction.text()));
            }
            (api.end_disabled)(ctx);
            if busy {
                same_line();
                dim(c"... l'IA réfléchit");
            }

            (api.separator_text)(ctx, c"CONSIGNE POUR L'IA (facultative)".as_ptr());
            let mut height = 56.0f64;
            (api.input_text_multiline)(
                ctx,
                c"##consigne".as_ptr(),
                self.instruction.pointer(),
                TEXT_CAPACITY as c_int,
                &mut fill,
                &mut height,
                null_mut(),
                null_mut(),
            );

            (api.separator_text)(ctx, c"RÉGLAGES DE L'IA".as_ptr());
            (api.set_next_item_width)(ctx, -90.0);
            (api.input_text)(
                ctx,
                c"adresse".as_ptr(),
                self.base_url.pointer(),
                TEXT_CAPACITY as c_int,
                null_mut(),
                null_mut(),
            );
            (api.set_next_item_width)(ctx, -90.0);
            (api.input_text)(
                ctx,
                c"modèle".as_ptr(),
                self.model.pointer(),
                TEXT_CAPACITY as c_int,
                null_mut(),
                null_mut(),
            );
            let mut password = api.password;
            (api.set_next_item_width)(ctx, -90.0);
            (api.input_text)(
                ctx,
                c"clé".as_ptr(),
                self.api_key.pointer(),
                TEXT_CAPACITY as c_int,
                &mut password,
                null_mut(),
            );
            if button(c"ENREGISTRER") {
                commands.push(Command::SaveSettings(AiSettings {
                    base_url: self.base_url.text(),
                    model: self.model.text(),
                    api_key: self.api_key.text(),
                }));
            }
            same_line();
            let mut disabled = busy;
            (api.begin_disabled)(ctx, &mut disabled);
            if button(c"LISTER LES MODÈLES") {
                commands.push(Command::ListModels);
            }
            (api.end_disabled)(ctx);

            let mut picked = None;
            if !self.models.is_empty() {
                dim(c"Clique sur un modèle pour le choisir, puis ENREGISTRER :");
                let (mut width, mut height) = (0.0f64, 110.0f64);
                let mut borders = api.child_borders;
                if (api.begin_child)(
                    ctx,
                    c"##modeles".as_ptr(),
                    &mut width,
                    &mut height,
                    &mut borders,
                    null_mut(),
                ) {
                    for name in &self.models {
                        let label = c_string(name);
                        if (api.selectable)(
                            ctx,
                            label.as_ptr(),
                            null_mut(),
                            null_mut(),
                            null_mut(),
                            null_mut(),
                        ) {
                            picked = Some(name.clone());
                        }
                    }
                    (api.end_child)(ctx);
                }
            }

            (api.separator_text)(ctx, c"JOURNAL".as_ptr());
            let (mut width, mut height) = (0.0f64, 0.0f64);
            let mut borders = api.child_borders;
            if (api.begin_child)(
                ctx,
                c"##journal".as_ptr(),
                &mut width,
                &mut height,
                &mut borders,
                null_mut(),
            ) {
                let log = LOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                for line in &log.1 {
                    let line = c_string(line);
                    (api.text)(ctx, line.as_ptr());
                }
                // Une nouvelle ligne est arrivée : on descend la montrer.
                if log.0 != self.log_seen {
                    self.log_seen = log.0;
                    let mut bottom = 1.0f64;
                    (api.set_scroll_here_y)(ctx, &mut bottom);
                }
                (api.end_child)(ctx);
            }

            if let Some(name) = picked {
                self.model.set(&name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_field_round_trips_and_trims() {
        let mut field = TextField::new("  plus lourd, en demi-temps ");
        assert_eq!(field.text(), "plus lourd, en demi-temps");
        field.set("");
        assert_eq!(field.text(), "");
    }

    #[test]
    fn text_field_cuts_long_text_on_a_character_boundary() {
        let long = "é".repeat(TEXT_CAPACITY);
        let field = TextField::new(&long);
        let text = field.text();
        assert!(text.len() < TEXT_CAPACITY);
        assert!(text.chars().all(|c| c == 'é'), "caractère coupé en deux");
    }

    #[test]
    fn log_keeps_the_last_lines_and_counts_them_all() {
        let before = LOG.lock().unwrap().0;
        log("première\ndeuxième");
        let log = LOG.lock().unwrap();
        assert_eq!(log.0, before + 2);
        assert_eq!(log.1.back().map(String::as_str), Some("deuxième"));
        assert!(log.1.len() <= LOG_LINES);
    }
}
