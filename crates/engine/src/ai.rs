//! Appel à une IA par une API compatible OpenAI (OpenRouter, OpenAI, ou toute autre adresse
//! qui parle le même protocole).
//!
//! L'IA ne voit pas l'audio et n'écrit pas de notes. Elle reçoit le compte rendu chiffré du
//! passage (tempo, motifs, niveaux, batterie existante) et choisit un `DrumPlan` ; le moteur
//! écrit ensuite les notes.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::drums::DrumPlan;
use crate::report::{selection_report, SelectionAnalysis};
use crate::styles::{overlay_plan, Style};

pub const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1";
pub const OPENAI_URL: &str = "https://api.openai.com/v1";

/// Délai maximal d'une réponse. Certains modèles mettent plusieurs dizaines de secondes.
const TIMEOUT: Duration = Duration::from_secs(90);
/// Longueur maximale d'un extrait de réponse cité dans un message d'erreur.
const ERROR_EXCERPT: usize = 300;

/// Réglages de l'IA, tels qu'ils sont enregistrés dans le fichier de réglages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    /// Adresse de l'API, par exemple `https://openrouter.ai/api/v1`.
    pub base_url: String,
    /// Identifiant du modèle chez ce fournisseur.
    pub model: String,
    /// Clé d'accès. Jamais affichée ni écrite dans un journal.
    pub api_key: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            base_url: OPENROUTER_URL.to_string(),
            model: String::new(),
            api_key: String::new(),
        }
    }
}

impl AiSettings {
    /// Lit des réglages en JSON. Une clé absente du fichier peut venir de `fallback_key`
    /// (variable d'environnement).
    pub fn from_json(text: &str, fallback_key: Option<String>) -> Result<AiSettings, String> {
        let mut settings: AiSettings =
            serde_json::from_str(text).map_err(|e| format!("réglages illisibles : {e}"))?;
        settings.base_url = settings.base_url.trim().trim_end_matches('/').to_string();
        settings.model = settings.model.trim().to_string();
        settings.api_key = settings.api_key.trim().to_string();
        if settings.api_key.is_empty() {
            settings.api_key = fallback_key.unwrap_or_default().trim().to_string();
        }
        Ok(settings)
    }

    /// Le fichier de réglages de départ, à compléter.
    pub fn template_json() -> String {
        serde_json::to_string_pretty(&AiSettings::default()).expect("réglages par défaut")
    }

    fn url(&self, path: &str) -> Result<String, String> {
        if !(self.base_url.starts_with("https://") || self.base_url.starts_with("http://")) {
            return Err(format!(
                "l'adresse de l'API doit commencer par https:// (réglage base_url : « {} »)",
                self.base_url
            ));
        }
        Ok(format!("{}/{path}", self.base_url))
    }

    fn agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new().timeout(TIMEOUT).build()
    }

    fn authorize(&self, request: ureq::Request) -> ureq::Request {
        if self.api_key.is_empty() {
            request
        } else {
            request.set("Authorization", &format!("Bearer {}", self.api_key))
        }
    }
}

const SYSTEM_PROMPT: &str = "\
Tu es un batteur de studio expérimenté. On te donne le compte rendu chiffré d'un passage d'un \
morceau en cours de composition : tempo, mesure, motif rythmique dominant de chaque piste \
(un groupe par noire ; X accent fort, x accent léger, . rien), mise en place, niveau sonore par \
mesure, et la batterie existante s'il y en a une. Tu ne reçois pas l'audio.

Tu choisis comment la batterie doit jouer ce passage. Un moteur écrira les notes à partir de tes \
choix : la grosse caisse sera calée sur les accents du morceau.

Réponds uniquement par un objet JSON, sans texte autour, avec ces champs :
- \"style\" : le style visé, en quelques mots, en français ;
- \"comment\" : deux phrases au plus, en français, qui expliquent tes choix au musicien ;
- \"feel\" : \"normal\" (caisse claire sur 2 et 4), \"half_time\" (caisse claire sur 3), \
\"double_time\" (caisse claire sur chaque contretemps) ou \"blast\" (blast beat : grosse caisse \
et caisse claire en alternance à la double croche) ;
- \"hat\" : \"eighths\" (charleston fermé à la croche), \"sixteenths\" (à la double croche), \
\"quarters\" (à la noire), \"open\" (charleston ouvert à la croche), \"ride\" (ride à la croche) \
ou \"none\" ;
- \"kick\" : \"accents\" (grosse caisse sur les accents du morceau), \"four_on_floor\" (sur \
chaque temps, plus les accents) ou \"double_bass\" (double pédale en doubles croches continues) ;
- \"kick_density\" : nombre de 0 à 1 ; 0 = grosse caisse sur le premier temps seulement, \
0.5 = sur les accents forts, 1 = sur tous les accents ;
- \"ghost_notes\" : true ou false, notes fantômes de caisse claire ;
- \"crash\" : true ou false, crash sur le premier temps du passage ;
- \"fill\" : true ou false, roulement à la fin du passage ;
- \"fill_every\" : nombre entier ; roulement toutes les N mesures, suivi d'un crash (0 = jamais) ;
- \"intensity\" : nombre de 0 à 1, force de frappe générale.

Si le musicien donne une consigne, elle passe avant ton propre goût.";

/// Le message envoyé à l'IA : le compte rendu du passage, les niveaux par mesure et la consigne.
pub fn user_prompt(analysis: &SelectionAnalysis, instruction: Option<&str>) -> String {
    let mut text = selection_report(analysis);
    if !analysis.tracks.is_empty() {
        text.push_str("\nNiveau sonore par mesure (0 à 1), piste par piste :\n");
        for track in &analysis.tracks {
            let levels: Vec<String> = track
                .accents
                .bars
                .iter()
                .map(|bar| format!("{:.1}", bar.energy))
                .collect();
            text.push_str(&format!("  {} : {}\n", track.name, levels.join(" ")));
        }
    }
    if analysis.tracks.is_empty() {
        text.push_str(
            "\nLe passage est vide : le musicien compose à partir de zéro. Propose la batterie de départ ; elle sera posée sur un motif de base (premier temps, temps 3, contretemps).\n",
        );
    }
    match instruction.map(str::trim).filter(|i| !i.is_empty()) {
        Some(instruction) => text.push_str(&format!("\nConsigne du musicien : {instruction}\n")),
        None => text.push_str("\nPas de consigne du musicien.\n"),
    }
    text
}

/// Les instructions données à l'IA pour choisir un plan : les instructions générales, puis la
/// pré-consigne du style s'il y en a un.
pub fn plan_system_prompt(style: Option<&Style>) -> String {
    match style {
        Some(style) => format!("{SYSTEM_PROMPT}\n\n{}", style.preprompt()),
        None => SYSTEM_PROMPT.to_string(),
    }
}

/// Lit le plan de batterie dans la réponse de l'IA. Tolère du texte autour de l'objet JSON
/// (certaines IA l'entourent de phrases ou d'une clôture de code).
pub fn parse_plan(answer: &str) -> Result<DrumPlan, String> {
    parse_plan_on(answer, &DrumPlan::default())
}

/// Comme `parse_plan`, mais un champ absent de la réponse garde la valeur de `base` (le jeu de
/// départ du style) au lieu de la valeur par défaut.
pub fn parse_plan_on(answer: &str, base: &DrumPlan) -> Result<DrumPlan, String> {
    let (Some(start), Some(end)) = (answer.find('{'), answer.rfind('}')) else {
        return Err(format!(
            "la réponse de l'IA ne contient pas d'objet JSON : « {} »",
            excerpt(answer)
        ));
    };
    serde_json::from_str::<Value>(&answer[start..=end])
        .map_err(|e| e.to_string())
        .and_then(|changes| overlay_plan(base, &changes))
        .map_err(|e| {
            format!(
                "réponse de l'IA inutilisable ({e}) : « {} »",
                excerpt(answer)
            )
        })
}

/// Demande à l'IA comment jouer le passage, dans le style choisi s'il y en a un. Bloque jusqu'à
/// la réponse : à appeler hors du fil principal de REAPER.
pub fn ask_plan(
    settings: &AiSettings,
    analysis: &SelectionAnalysis,
    style: Option<&Style>,
    instruction: Option<&str>,
) -> Result<DrumPlan, String> {
    let base = style.map(Style::base_plan).unwrap_or_default();
    parse_plan_on(
        &chat(
            settings,
            &plan_system_prompt(style),
            &user_prompt(analysis, instruction),
        )?,
        &base,
    )
}

const ADVICE_PROMPT: &str = "\
Tu es un batteur de studio expérimenté. On te donne le compte rendu chiffré d'un passage d'un \
morceau en cours de composition : tempo, mesure, motif rythmique dominant de chaque piste \
(un groupe par noire ; X accent fort, x accent léger, . rien), mise en place, niveau sonore par \
mesure, la batterie MIDI existante élément par élément, et les remarques déjà tirées par un \
programme de la comparaison entre cette batterie et les accents du morceau. Tu ne reçois pas \
l'audio.

Donne au musicien au plus cinq conseils pour améliorer cette batterie : ce qu'il faut changer, \
à quel endroit, et pourquoi. Sois concret (élément, temps, mesure) et ne répète pas les remarques \
du programme. Réponds en français, en texte simple, un conseil par ligne commençant par un \
tiret, sans titre ni mise en forme. Si le musicien pose une question ou donne une consigne, \
réponds-y d'abord.";

/// Demande à l'IA son avis sur la batterie existante du passage. `findings` : les conseils déjà
/// tirés par le moteur, en texte ; `style` : le style visé, s'il y en a un. Bloque jusqu'à la
/// réponse.
pub fn ask_advice(
    settings: &AiSettings,
    analysis: &SelectionAnalysis,
    findings: &str,
    style: Option<&Style>,
    instruction: Option<&str>,
) -> Result<String, String> {
    let mut user = user_prompt(analysis, instruction);
    user.push_str("\nRemarques du programme :\n");
    user.push_str(findings);
    let system = match style {
        Some(style) => format!("{ADVICE_PROMPT}\n\n{}", style.advice_context()),
        None => ADVICE_PROMPT.to_string(),
    };
    let answer = chat(settings, &system, &user)?;
    let answer = answer.trim();
    if answer.is_empty() {
        return Err("l'IA a répondu par un texte vide".to_string());
    }
    Ok(answer.to_string())
}

/// Envoie un échange système + utilisateur et rend le texte de la réponse.
fn chat(settings: &AiSettings, system: &str, user: &str) -> Result<String, String> {
    if settings.model.is_empty() {
        return Err("aucun modèle choisi (réglage model)".to_string());
    }
    let body = json!({
        "model": settings.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });
    let request = settings.authorize(settings.agent().post(&settings.url("chat/completions")?));
    let response: Value = read_json(request.send_json(body))?;
    response["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "réponse inattendue du fournisseur : « {} »",
                excerpt(&response.to_string())
            )
        })
}

/// Liste les identifiants des modèles proposés par le fournisseur, triés.
pub fn list_models(settings: &AiSettings) -> Result<Vec<String>, String> {
    let request = settings.authorize(settings.agent().get(&settings.url("models")?));
    let response: Value = read_json(request.call())?;
    let mut models: Vec<String> = response["data"]
        .as_array()
        .ok_or_else(|| {
            format!(
                "réponse inattendue du fournisseur : « {} »",
                excerpt(&response.to_string())
            )
        })?
        .iter()
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect();
    models.sort();
    Ok(models)
}

fn read_json(result: Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
    match result {
        Ok(response) => response
            .into_json()
            .map_err(|e| format!("réponse illisible du fournisseur : {e}")),
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            let hint = match code {
                401 | 403 => " Vérifie la clé d'accès (réglage api_key).",
                404 => " Vérifie l'adresse (base_url) et le nom du modèle (model).",
                429 => " Trop de demandes ou crédit épuisé chez le fournisseur.",
                _ => "",
            };
            Err(format!(
                "le fournisseur a répondu {code} : « {} ».{hint}",
                excerpt(&body)
            ))
        }
        Err(ureq::Error::Transport(e)) => Err(format!("connexion au fournisseur impossible : {e}")),
    }
}

fn excerpt(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > ERROR_EXCERPT {
        format!("{}…", flat.chars().take(ERROR_EXCERPT).collect::<String>())
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use super::*;
    use crate::accents::Grid;
    use crate::drums::{Feel, Hat, Kick};
    use crate::styles::builtin_styles;

    fn analysis() -> SelectionAnalysis {
        SelectionAnalysis {
            start_s: 0.0,
            end_s: 4.0,
            first_measure: 1,
            grid: Grid {
                bpm: 120.0,
                beats_per_bar: 4.0,
                slots_per_beat: 4,
                origin_s: 0.0,
            },
            tempo_changes: false,
            tracks: Vec::new(),
            silent_tracks: Vec::new(),
            drums: Vec::new(),
        }
    }

    /// Un faux fournisseur : répond une fois `status` et `body`, et rend la requête reçue.
    fn fake_provider(status: &'static str, body: String) -> (AiSettings, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(header_end) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + length || read == 0 {
                        break;
                    }
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&request).to_string());
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let settings = AiSettings {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            model: "test/modele".to_string(),
            api_key: "cle-de-test".to_string(),
        };
        (settings, receiver)
    }

    fn completion(content: &str) -> String {
        json!({ "choices": [{ "message": { "role": "assistant", "content": content } }] })
            .to_string()
    }

    #[test]
    fn asking_sends_the_model_the_key_and_the_instruction() {
        let (settings, request) = fake_provider(
            "200 OK",
            completion(
                r#"{"style":"rock lourd","feel":"half_time","hat":"ride","kick_density":0.9,"fill":true}"#,
            ),
        );
        let plan = ask_plan(
            &settings,
            &analysis(),
            None,
            Some("plus lourd sur le refrain"),
        )
        .unwrap();
        assert_eq!(plan.feel, Feel::HalfTime);
        assert_eq!(plan.hat, Hat::Ride);
        assert!(plan.fill);
        assert_eq!(plan.style, "rock lourd");
        // Les champs absents de la réponse gardent leur valeur par défaut.
        assert!(plan.crash);

        let request = request.recv().unwrap();
        assert!(
            request.starts_with("POST /v1/chat/completions "),
            "{request}"
        );
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer cle-de-test"),
            "{request}"
        );
        assert!(request.contains(r#""model":"test/modele""#), "{request}");
        assert!(request.contains("plus lourd sur le refrain"), "{request}");
    }

    fn style(id: &str) -> Style {
        builtin_styles().into_iter().find(|s| s.id == id).unwrap()
    }

    #[test]
    fn a_style_is_sent_before_the_passage_and_fills_what_the_ai_leaves_out() {
        // L'IA ne retouche que la pulsation : le reste vient du jeu de départ du style.
        let (settings, request) = fake_provider(
            "200 OK",
            completion(r#"{"feel":"blast","comment":"Trémolo : blast."}"#),
        );
        let death = style("death-metal");
        let plan = ask_plan(&settings, &analysis(), Some(&death), None).unwrap();
        assert_eq!(plan.feel, Feel::Blast);
        assert_eq!(plan.kick, Kick::DoubleBass);
        assert_eq!(plan.hat, Hat::Open);
        assert_eq!(plan.fill_every, 4);
        assert_eq!(plan.intensity, 1.0);
        assert_eq!(plan.style, "Death Metal");
        assert_eq!(plan.comment, "Trémolo : blast.");

        let request = request.recv().unwrap();
        // Le corps est du JSON : les accents y sont échappés ou non selon l'encodeur.
        assert!(request.contains("Death Metal"), "{request}");
        assert!(request.contains("double_bass"), "{request}");
        assert!(request.contains("Jeu de d"), "{request}");
    }

    #[test]
    fn without_a_style_the_instructions_stay_general() {
        assert!(!plan_system_prompt(None).contains("Style imposé"));
        let with = plan_system_prompt(Some(&style("indus")));
        assert!(with.starts_with(SYSTEM_PROMPT));
        assert!(with.contains("Style imposé par le musicien : Indus."));
    }

    #[test]
    fn advice_is_asked_for_the_chosen_style() {
        let (settings, request) = fake_provider("200 OK", completion("- Ajoute un blast."));
        let death = style("death-metal");
        ask_advice(&settings, &analysis(), "rien", Some(&death), None).unwrap();
        let request = request.recv().unwrap();
        assert!(request.contains("vise le style Death Metal"), "{request}");
    }

    #[test]
    fn advice_request_carries_the_findings_and_returns_the_text() {
        let (settings, request) = fake_provider(
            "200 OK",
            completion("- Ouvre le charleston sur le dernier temps.\n- Allège la mesure 3.\n"),
        );
        let advice = ask_advice(
            &settings,
            &analysis(),
            "1. Appuyer 2 accents forts",
            None,
            Some("trop chargé ?"),
        )
        .unwrap();
        assert_eq!(
            advice,
            "- Ouvre le charleston sur le dernier temps.\n- Allège la mesure 3."
        );
        let request = request.recv().unwrap();
        assert!(request.contains("Remarques du programme"), "{request}");
        assert!(request.contains("Appuyer 2 accents forts"), "{request}");
        assert!(request.contains("trop charg"), "{request}");
    }

    #[test]
    fn a_refused_key_gives_a_message_that_says_what_to_check() {
        let (settings, _request) = fake_provider(
            "401 Unauthorized",
            r#"{"error":{"message":"No auth credentials found"}}"#.to_string(),
        );
        let error = ask_plan(&settings, &analysis(), None, None).unwrap_err();
        assert!(
            error.contains("401") && error.contains("api_key"),
            "{error}"
        );
        assert!(
            !error.contains("cle-de-test"),
            "la clé ne doit pas apparaître : {error}"
        );
    }

    #[test]
    fn models_are_listed_and_sorted() {
        let (settings, request) = fake_provider(
            "200 OK",
            r#"{"data":[{"id":"zeta/grand"},{"id":"alpha/petit"}]}"#.to_string(),
        );
        assert_eq!(
            list_models(&settings).unwrap(),
            vec!["alpha/petit", "zeta/grand"]
        );
        assert!(request.recv().unwrap().starts_with("GET /v1/models "));
    }

    #[test]
    fn asking_without_a_model_fails_before_any_request() {
        let settings = AiSettings {
            model: String::new(),
            ..AiSettings::default()
        };
        assert!(ask_plan(&settings, &analysis(), None, None)
            .unwrap_err()
            .contains("model"));
    }

    #[test]
    fn an_unreachable_provider_is_reported_as_such() {
        let settings = AiSettings {
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        };
        assert!(ask_plan(&settings, &analysis(), None, None)
            .unwrap_err()
            .contains("connexion"));
    }

    #[test]
    fn plan_is_read_even_with_text_around_the_json() {
        let answer = "Voici mon choix :\n```json\n{\"hat\": \"sixteenths\", \"intensity\": 1.7}\n```\nBonne session !";
        let plan = parse_plan(answer).unwrap();
        assert_eq!(plan.hat, Hat::Sixteenths);
        assert_eq!(plan.intensity, 1.0);
    }

    #[test]
    fn an_answer_without_json_or_with_an_unknown_choice_is_an_error() {
        assert!(parse_plan("Je ne sais pas.").unwrap_err().contains("JSON"));
        assert!(parse_plan(r#"{"hat": "cloche"}"#)
            .unwrap_err()
            .contains("inutilisable"));
    }

    #[test]
    fn settings_are_cleaned_and_the_key_can_come_from_the_environment() {
        let settings = AiSettings::from_json(
            r#"{"base_url": " https://api.openai.com/v1/ ", "model": " un-modele "}"#,
            Some("cle-env".to_string()),
        )
        .unwrap();
        assert_eq!(settings.base_url, OPENAI_URL);
        assert_eq!(settings.model, "un-modele");
        assert_eq!(settings.api_key, "cle-env");
        // La clé du fichier passe avant celle de l'environnement.
        let settings =
            AiSettings::from_json(r#"{"api_key": "cle-fichier"}"#, Some("cle-env".to_string()))
                .unwrap();
        assert_eq!(settings.api_key, "cle-fichier");
        assert_eq!(settings.base_url, OPENROUTER_URL);
    }

    #[test]
    fn the_template_is_valid_and_has_no_key() {
        let settings = AiSettings::from_json(&AiSettings::template_json(), None).unwrap();
        assert_eq!(settings, AiSettings::default());
    }

    #[test]
    fn the_prompt_carries_the_instruction_or_says_there_is_none() {
        assert!(user_prompt(&analysis(), Some("  plus calme "))
            .contains("Consigne du musicien : plus calme"));
        assert!(user_prompt(&analysis(), Some("  ")).contains("Pas de consigne"));
        // Sans piste audio, l'IA est prévenue qu'on compose à partir de zéro.
        assert!(user_prompt(&analysis(), None).contains("à partir de zéro"));
    }
}
