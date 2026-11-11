//! Styles de batterie : un jeu de départ et une pré-consigne pour l'IA.
//!
//! Un style fait deux choses. Sans IA, son jeu de départ (`plan`) remplace les choix par défaut
//! de l'ébauche. Avec l'IA, sa pré-consigne (`prompt`), son jeu de départ et ses exemples sont
//! ajoutés aux instructions : l'IA part du style et ne retouche que ce que le passage demande.
//!
//! Les styles fournis sont dans `crates/engine/styles/*.json`. L'utilisateur peut en ajouter au
//! même format.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::drums::DrumPlan;

/// Un exemple de jeu dans un style : une situation et les choix qui lui vont.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleExample {
    /// La situation, en une phrase (« Riff en trémolo, niveau au maximum »).
    pub situation: String,
    /// Les choix pour cette situation. Seuls les champs qui changent par rapport au jeu de
    /// départ du style sont nécessaires.
    pub plan: Value,
}

/// Un style de batterie.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    /// Identifiant court, sans espace (« death-metal »).
    pub id: String,
    /// Nom affiché.
    pub name: String,
    /// Le style en une phrase, pour l'affichage.
    pub description: String,
    /// La pré-consigne donnée à l'IA : les règles de jeu du style.
    pub prompt: String,
    /// Le jeu de départ du style.
    pub plan: DrumPlan,
    pub examples: Vec<StyleExample>,
}

const BUILTIN: [&str; 8] = [
    include_str!("../styles/rock.json"),
    include_str!("../styles/punk.json"),
    include_str!("../styles/funk.json"),
    include_str!("../styles/metal.json"),
    include_str!("../styles/death-metal.json"),
    include_str!("../styles/indus.json"),
    include_str!("../styles/prog.json"),
    include_str!("../styles/doom.json"),
];

/// Le fichier de style à copier pour écrire le sien.
pub const TEMPLATE_JSON: &str = include_str!("../styles/death-metal.json");

/// Les styles fournis avec AI Drummer.
pub fn builtin_styles() -> Vec<Style> {
    BUILTIN
        .iter()
        .map(|text| Style::from_json(text).expect("style fourni valide"))
        .collect()
}

impl Style {
    /// Lit un style en JSON. Le nom sert d'identifiant s'il n'y en a pas ; chaque exemple doit
    /// donner des choix que le moteur comprend.
    pub fn from_json(text: &str) -> Result<Style, String> {
        let mut style: Style =
            serde_json::from_str(text).map_err(|e| format!("style illisible : {e}"))?;
        style.id = slug(if style.id.trim().is_empty() {
            &style.name
        } else {
            &style.id
        });
        style.name = style.name.trim().to_string();
        if style.id.is_empty() {
            return Err("style sans nom : renseigne « name »".to_string());
        }
        if style.name.is_empty() {
            style.name = style.id.clone();
        }
        style.plan = style.plan.clamped();
        for example in &style.examples {
            overlay_plan(&style.plan, &example.plan).map_err(|e| {
                format!(
                    "style « {} », exemple « {} » : {e}",
                    style.name, example.situation
                )
            })?;
        }
        Ok(style)
    }

    /// Le jeu de départ du style, prêt à être écrit : il porte le nom du style.
    pub fn base_plan(&self) -> DrumPlan {
        DrumPlan {
            style: self.name.clone(),
            ..self.plan.clone()
        }
    }

    /// Le bloc ajouté aux instructions de l'IA quand ce style est choisi.
    pub fn preprompt(&self) -> String {
        let mut text = format!("Style imposé par le musicien : {}.", self.name);
        for part in [&self.description, &self.prompt] {
            if !part.trim().is_empty() {
                text.push(' ');
                text.push_str(part.trim());
            }
        }
        text.push_str(&format!(
            "\n\nJeu de départ de ce style :\n{}\nPars de ce jeu de départ. Ne change que les champs que le passage ou la consigne du musicien demandent de changer, et reste dans le style. Un champ que tu ne donnes pas garde la valeur du jeu de départ.",
            choices(&self.plan)
        ));
        if !self.examples.is_empty() {
            text.push_str("\n\nExemples dans ce style (situation → choix) :");
            for example in &self.examples {
                text.push_str(&format!(
                    "\n- {} → {}",
                    example.situation.trim(),
                    example.plan
                ));
            }
        }
        text
    }

    /// Le style en une phrase, pour situer l'avis de l'IA sur une batterie existante.
    pub fn advice_context(&self) -> String {
        let mut text = format!(
            "Le musicien vise le style {} : juge la batterie par rapport à ce style et conseille dans ce sens.",
            self.name
        );
        for part in [&self.description, &self.prompt] {
            if !part.trim().is_empty() {
                text.push(' ');
                text.push_str(part.trim());
            }
        }
        text
    }
}

/// Les choix de jeu d'un plan en JSON, sans les champs d'affichage.
fn choices(plan: &DrumPlan) -> String {
    let mut value = serde_json::to_value(plan).expect("plan sérialisable");
    if let Some(fields) = value.as_object_mut() {
        fields.remove("style");
        fields.remove("comment");
    }
    value.to_string()
}

/// Pose les champs donnés dans `changes` sur le plan `base`. Les champs absents ou nuls gardent
/// la valeur de `base`.
pub fn overlay_plan(base: &DrumPlan, changes: &Value) -> Result<DrumPlan, String> {
    let Some(changes) = changes.as_object() else {
        return Err("les choix doivent être un objet JSON".to_string());
    };
    let mut value = serde_json::to_value(base).expect("plan sérialisable");
    let fields = value.as_object_mut().expect("un plan est un objet");
    for (name, change) in changes {
        if !change.is_null() {
            fields.insert(name.clone(), change.clone());
        }
    }
    serde_json::from_value::<DrumPlan>(value)
        .map(DrumPlan::clamped)
        .map_err(|e| e.to_string())
}

/// Réunit les styles fournis et ceux de l'utilisateur. Un style de l'utilisateur remplace le
/// style fourni de même identifiant ; les autres s'ajoutent à la suite.
pub fn merge_styles(user: Vec<Style>) -> Vec<Style> {
    let mut styles = builtin_styles();
    for style in user {
        match styles.iter_mut().find(|s| s.id == style.id) {
            Some(existing) => *existing = style,
            None => styles.push(style),
        }
    }
    styles
}

/// Identifiant court : minuscules, chiffres et tirets.
fn slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accents::{template_accents, Grid};
    use crate::drums::{write_drums, Feel, Hat, Kick};

    fn grid() -> Grid {
        Grid {
            bpm: 120.0,
            beats_per_bar: 4.0,
            slots_per_beat: 4,
            origin_s: 0.0,
        }
    }

    #[test]
    fn builtin_styles_are_valid_distinct_and_complete() {
        let styles = builtin_styles();
        let mut ids: Vec<&str> = styles.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"death-metal") && ids.contains(&"indus") && ids.contains(&"prog"));
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), styles.len());
        for style in &styles {
            assert!(!style.name.is_empty() && !style.description.is_empty());
            assert!(!style.prompt.is_empty(), "{}", style.id);
            assert!(style.examples.len() >= 3, "{}", style.id);
        }
    }

    #[test]
    fn each_builtin_style_writes_its_own_drums() {
        // Sur le même passage vide, deux styles ne donnent jamais les mêmes notes.
        let template = template_accents(grid(), 16.0);
        let parts: Vec<(String, String)> = builtin_styles()
            .iter()
            .map(|style| {
                let part = write_drums(&grid(), std::slice::from_ref(&template), &style.plan);
                assert!(!part.is_empty(), "{}", style.id);
                (style.id.clone(), format!("{part:?}"))
            })
            .collect();
        for (index, (id, part)) in parts.iter().enumerate() {
            for (other, other_part) in &parts[index + 1..] {
                assert_ne!(
                    part, other_part,
                    "{id} et {other} écrivent la même batterie"
                );
            }
        }
    }

    #[test]
    fn death_metal_starts_on_double_bass_and_indus_on_four_on_the_floor() {
        let styles = builtin_styles();
        let style = |id: &str| styles.iter().find(|s| s.id == id).unwrap();
        assert_eq!(style("death-metal").plan.kick, Kick::DoubleBass);
        assert_eq!(style("death-metal").plan.intensity, 1.0);
        assert_eq!(style("indus").plan.kick, Kick::FourOnFloor);
        assert_eq!(style("indus").plan.hat, Hat::Sixteenths);
        assert!(style("prog").plan.ghost_notes);
        assert_eq!(style("doom").plan.feel, Feel::HalfTime);
    }

    #[test]
    fn preprompt_carries_the_rules_the_base_plan_and_the_examples() {
        let styles = builtin_styles();
        let style = styles.iter().find(|s| s.id == "death-metal").unwrap();
        let text = style.preprompt();
        assert!(
            text.starts_with("Style imposé par le musicien : Death Metal."),
            "{text}"
        );
        assert!(text.contains("blast beat"), "{text}");
        assert!(text.contains(r#""kick":"double_bass""#), "{text}");
        assert!(text.contains("Riff en trémolo"), "{text}");
        assert!(text.contains(r#""feel":"blast""#), "{text}");
        // Les champs d'affichage ne sont pas donnés comme des choix de jeu.
        assert!(!text.contains(r#""comment""#), "{text}");
    }

    #[test]
    fn overlay_keeps_the_base_where_nothing_is_given() {
        let base = DrumPlan {
            kick: Kick::DoubleBass,
            intensity: 1.0,
            ..DrumPlan::default()
        };
        let plan = overlay_plan(
            &base,
            &serde_json::json!({"feel": "blast", "hat": null, "intensity": 3}),
        )
        .unwrap();
        assert_eq!(plan.feel, Feel::Blast);
        assert_eq!(plan.kick, Kick::DoubleBass);
        assert_eq!(plan.hat, Hat::Eighths);
        assert_eq!(plan.intensity, 1.0);
        assert!(overlay_plan(&base, &serde_json::json!({"kick": "triple"})).is_err());
        assert!(overlay_plan(&base, &serde_json::json!("blast")).is_err());
    }

    #[test]
    fn a_user_style_needs_only_a_name() {
        let style =
            Style::from_json(r#"{"name": " Black Metal Atmo ", "prompt": "Blast."}"#).unwrap();
        assert_eq!(style.id, "black-metal-atmo");
        assert_eq!(style.name, "Black Metal Atmo");
        assert_eq!(style.plan, DrumPlan::default());
        assert_eq!(style.base_plan().style, "Black Metal Atmo");
        assert!(Style::from_json("{}").unwrap_err().contains("name"));
    }

    #[test]
    fn a_user_style_with_a_wrong_example_says_which_one() {
        let error = Style::from_json(
            r#"{"name": "Essai", "examples": [{"situation": "Refrain", "plan": {"hat": "cloche"}}]}"#,
        )
        .unwrap_err();
        assert!(
            error.contains("Essai") && error.contains("Refrain"),
            "{error}"
        );
    }

    #[test]
    fn user_styles_replace_or_extend_the_builtin_ones() {
        let custom = Style::from_json(r#"{"id": "indus", "name": "Indus maison"}"#).unwrap();
        let new = Style::from_json(r#"{"name": "Shoegaze"}"#).unwrap();
        let styles = merge_styles(vec![custom, new]);
        assert_eq!(styles.len(), builtin_styles().len() + 1);
        assert_eq!(
            styles.iter().find(|s| s.id == "indus").unwrap().name,
            "Indus maison"
        );
        assert_eq!(styles.last().unwrap().id, "shoegaze");
    }
}
