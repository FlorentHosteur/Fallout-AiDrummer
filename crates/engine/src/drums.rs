//! Batterie : table de notes, lecture d'une partie existante, ébauche d'une partie nouvelle.
//!
//! Les positions sont en noires, comptées depuis le début de la mesure 0 de la grille
//! (celle qui contient le début du passage).

use serde::{Deserialize, Serialize};

use crate::accents::{AccentMap, Grid};

/// Un élément de la batterie, tel qu'on le lit dans une partie MIDI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Piece {
    Kick,
    Snare,
    HatClosed,
    HatOpen,
    Tom,
    Crash,
    Ride,
    /// Splash, china.
    Cymbal,
    /// Note que la table ne connaît pas.
    Other,
}

impl Piece {
    pub fn label(self) -> &'static str {
        match self {
            Piece::Kick => "grosse caisse",
            Piece::Snare => "caisse claire",
            Piece::HatClosed => "charleston fermé",
            Piece::HatOpen => "charleston ouvert",
            Piece::Tom => "toms",
            Piece::Crash => "crash",
            Piece::Ride => "ride",
            Piece::Cymbal => "splash / china",
            Piece::Other => "autres notes",
        }
    }
}

/// Notes écrites par l'ébauche. Elles sont communes à General MIDI et à la table Toontrack
/// (EZdrummer 3, Superior Drummer 3).
pub mod notes {
    pub const KICK: u8 = 36;
    pub const SNARE: u8 = 38;
    pub const HAT_CLOSED: u8 = 42;
    pub const HAT_OPEN: u8 = 46;
    pub const TOM_FLOOR: u8 = 43;
    pub const TOM_MID: u8 = 45;
    pub const CRASH: u8 = 49;
    pub const RIDE: u8 = 51;
}

/// À quel élément correspond une note MIDI.
///
/// Table Toontrack (EZdrummer 3, Superior Drummer 3) complétée par General MIDI. Les
/// articulations de charleston 22, 24-26 et 60-63 viennent du projet midi-drums (licence MIT) ;
/// elles n'ont pas été vérifiées dans Superior Drummer. Une note inconnue est rangée dans
/// `Other` plutôt que devinée.
pub fn piece_of(note: u8) -> Piece {
    match note {
        35 | 36 => Piece::Kick,
        37 | 38 | 40 => Piece::Snare,
        22 | 42 | 44 | 61 | 62 | 63 => Piece::HatClosed,
        24 | 25 | 26 | 46 | 60 => Piece::HatOpen,
        41 | 43 | 45 | 47 | 48 | 50 => Piece::Tom,
        49 | 57 => Piece::Crash,
        51 | 53 | 59 => Piece::Ride,
        52 | 55 => Piece::Cymbal,
        _ => Piece::Other,
    }
}

/// Un coup de batterie.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DrumNote {
    /// Position en noires depuis le début de la mesure 0 de la grille.
    pub position_qn: f64,
    pub note: u8,
    /// Vélocité MIDI, de 1 à 127.
    pub velocity: u8,
}

/// Le motif d'un élément de la batterie sur le passage.
#[derive(Debug, Clone, Serialize)]
pub struct DrumLane {
    pub piece: Piece,
    pub hits: usize,
    /// Part des mesures où chaque case est jouée, entre 0 et 1.
    pub folded: Vec<f32>,
    /// `folded` en texte : `X` joué dans la plupart des mesures, `x` dans certaines, `.` jamais ou presque.
    pub pattern: String,
}

/// Une partie de batterie lue sur le passage.
#[derive(Debug, Clone, Serialize)]
pub struct DrumReading {
    pub track: String,
    pub lanes: Vec<DrumLane>,
    /// Notes que la table ne connaît pas, avec leur nombre de coups.
    pub unknown_notes: Vec<(u8, usize)>,
    /// Part des accents forts du morceau qui tombent sur un coup de grosse caisse ou de caisse
    /// claire. `None` s'il n'y a aucun accent fort.
    pub accents_covered: Option<f32>,
    /// Part des coups de grosse caisse qui tombent sur un accent du morceau.
    /// `None` s'il n'y a pas de grosse caisse.
    pub kicks_on_accents: Option<f32>,
}

const OFTEN: f32 = 0.6;
const SOMETIMES: f32 = 0.25;
/// Un accent du morceau compte comme fort à partir de cette force.
pub(crate) const STRONG_ACCENT: f32 = 0.6;
pub(crate) const ANY_ACCENT: f32 = 0.25;

/// Case de la grille (numéro de mesure, case dans la mesure) la plus proche d'une position.
pub(crate) fn slot_of(position_qn: f64, grid: &Grid) -> (i64, usize) {
    let per_bar = grid.slots_per_bar() as i64;
    let global = (position_qn * grid.slots_per_beat as f64).round() as i64;
    (
        global.div_euclid(per_bar),
        global.rem_euclid(per_bar) as usize,
    )
}

/// Force de l'accent le plus fort du morceau dans chaque case de chaque mesure,
/// toutes pistes confondues.
pub(crate) fn combined_accents(tracks: &[AccentMap]) -> Vec<(i64, Vec<f32>)> {
    let Some(first) = tracks.first() else {
        return Vec::new();
    };
    first
        .bars
        .iter()
        .enumerate()
        .map(|(index, bar)| {
            let slots = (0..bar.slots.len())
                .map(|slot| {
                    tracks
                        .iter()
                        .filter_map(|t| t.bars.get(index).and_then(|b| b.slots.get(slot)))
                        .fold(0.0f32, |max, &s| max.max(s))
                })
                .collect();
            (bar.bar, slots)
        })
        .collect()
}

/// Lit une partie de batterie : motif de chaque élément, et accord avec les accents du morceau.
///
/// `bars` est la liste des mesures du passage (voir `AccentMap::bars`) ; `accents` les cartes
/// d'accents des pistes audio du même passage, sur la même grille.
pub fn read_drums(
    track: &str,
    part: &[DrumNote],
    grid: &Grid,
    accents: &[AccentMap],
) -> DrumReading {
    let combined = combined_accents(accents);
    let bar_count = combined.len().max(
        part.iter()
            .map(|n| slot_of(n.position_qn, grid).0)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
    );
    let per_bar = grid.slots_per_bar();

    let mut by_piece: std::collections::BTreeMap<Piece, Vec<&DrumNote>> = Default::default();
    let mut unknown: std::collections::BTreeMap<u8, usize> = Default::default();
    for note in part {
        let piece = piece_of(note.note);
        if piece == Piece::Other {
            *unknown.entry(note.note).or_default() += 1;
        }
        by_piece.entry(piece).or_default().push(note);
    }

    let lanes = by_piece
        .iter()
        .map(|(&piece, hits)| {
            let mut played: std::collections::BTreeSet<(i64, usize)> = Default::default();
            for hit in hits {
                played.insert(slot_of(hit.position_qn, grid));
            }
            let folded: Vec<f32> = (0..per_bar)
                .map(|slot| {
                    played.iter().filter(|(_, s)| *s == slot).count() as f32
                        / bar_count.max(1) as f32
                })
                .collect();
            DrumLane {
                piece,
                hits: hits.len(),
                pattern: lane_text(&folded, grid.slots_per_beat as usize),
                folded,
            }
        })
        .collect();

    let backbone: std::collections::BTreeSet<(i64, usize)> = part
        .iter()
        .filter(|n| matches!(piece_of(n.note), Piece::Kick | Piece::Snare))
        .map(|n| slot_of(n.position_qn, grid))
        .collect();
    let strong: Vec<(i64, usize)> = combined
        .iter()
        .flat_map(|(bar, slots)| {
            slots
                .iter()
                .enumerate()
                .filter(|(_, &s)| s >= STRONG_ACCENT)
                .map(move |(slot, _)| (*bar, slot))
        })
        .collect();
    let accents_covered = (!strong.is_empty()).then(|| {
        strong.iter().filter(|cell| backbone.contains(cell)).count() as f32 / strong.len() as f32
    });

    let kicks: Vec<(i64, usize)> = part
        .iter()
        .filter(|n| piece_of(n.note) == Piece::Kick)
        .map(|n| slot_of(n.position_qn, grid))
        .collect();
    let accent_at = |cell: &(i64, usize)| {
        combined
            .iter()
            .find(|(bar, _)| *bar == cell.0)
            .and_then(|(_, slots)| slots.get(cell.1))
            .copied()
            .unwrap_or(0.0)
    };
    let kicks_on_accents = (!kicks.is_empty() && !combined.is_empty()).then(|| {
        kicks
            .iter()
            .filter(|cell| accent_at(cell) >= ANY_ACCENT)
            .count() as f32
            / kicks.len() as f32
    });

    DrumReading {
        track: track.to_string(),
        lanes,
        unknown_notes: unknown.into_iter().collect(),
        accents_covered,
        kicks_on_accents,
    }
}

fn lane_text(folded: &[f32], slots_per_beat: usize) -> String {
    folded
        .chunks(slots_per_beat.max(1))
        .map(|beat| {
            beat.iter()
                .map(|&share| {
                    if share >= OFTEN {
                        'X'
                    } else if share >= SOMETIMES {
                        'x'
                    } else {
                        '.'
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rapport entre la pulsation de la batterie et celle du morceau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feel {
    /// Caisse claire sur les temps 2 et 4.
    #[default]
    Normal,
    /// Caisse claire sur le temps 3 seulement : la batterie paraît deux fois plus lente.
    HalfTime,
    /// Caisse claire sur chaque contretemps : la batterie paraît deux fois plus rapide.
    DoubleTime,
    /// Blast beat : grosse caisse et caisse claire en alternance, case après case.
    Blast,
}

/// Comment la grosse caisse est placée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kick {
    /// Sur le premier temps et sur les accents du morceau, selon `kick_density`.
    #[default]
    Accents,
    /// Sur chaque temps, plus les accents du morceau selon `kick_density`.
    FourOnFloor,
    /// Double pédale : sur chaque case de la grille (doubles croches en continu).
    DoubleBass,
}

/// Ce que joue la main droite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hat {
    /// Charleston fermé à la croche.
    #[default]
    Eighths,
    /// Charleston fermé à la double croche.
    Sixteenths,
    /// Charleston fermé à la noire.
    Quarters,
    /// Charleston ouvert à la croche.
    Open,
    /// Cymbale ride à la croche.
    Ride,
    /// Rien.
    None,
}

/// Les choix de jeu d'une partie de batterie. C'est ce que l'IA décide ; le moteur écrit les
/// notes à partir de ces choix et des accents du morceau.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrumPlan {
    /// Le style visé, en quelques mots. Pour l'affichage.
    pub style: String,
    /// Pourquoi ces choix. Pour l'affichage.
    pub comment: String,
    pub feel: Feel,
    pub hat: Hat,
    pub kick: Kick,
    /// De 0 à 1 : 0 = grosse caisse sur le premier temps seulement, 0,5 = sur les accents forts,
    /// 1 = sur tous les accents du morceau.
    pub kick_density: f32,
    /// Notes fantômes de caisse claire juste avant chaque coup de caisse claire.
    pub ghost_notes: bool,
    /// Crash sur le premier temps du passage.
    pub crash: bool,
    /// Roulement sur le dernier temps de la dernière mesure.
    pub fill: bool,
    /// Roulement toutes les N mesures, suivi d'un crash sur la mesure d'après. 0 = jamais.
    pub fill_every: u8,
    /// De 0 à 1 : force de frappe générale.
    pub intensity: f32,
}

impl Default for DrumPlan {
    fn default() -> Self {
        DrumPlan {
            style: String::new(),
            comment: String::new(),
            feel: Feel::Normal,
            hat: Hat::Eighths,
            kick: Kick::Accents,
            kick_density: 0.5,
            ghost_notes: false,
            crash: true,
            fill: false,
            fill_every: 0,
            intensity: 0.8,
        }
    }
}

impl DrumPlan {
    /// Ramène les valeurs dans leurs bornes (une IA peut répondre 1,5 ou -1).
    pub fn clamped(mut self) -> Self {
        let unit = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.5
            }
        };
        self.kick_density = unit(self.kick_density);
        self.intensity = unit(self.intensity);
        self
    }

    /// Les choix en une ligne, pour le journal.
    pub fn summary(&self) -> String {
        let yes_no = |value: bool| if value { "oui" } else { "non" };
        format!(
            "pulsation : {} · main droite : {} · grosse caisse : {} (densité {:.1}) · notes fantômes : {} · crash : {} · roulement : {} · intensité : {:.1}",
            match self.feel {
                Feel::Normal => "caisse claire sur 2 et 4",
                Feel::HalfTime => "demi-temps",
                Feel::DoubleTime => "tempo doublé",
                Feel::Blast => "blast beat",
            },
            match self.hat {
                Hat::Eighths => "charleston à la croche",
                Hat::Sixteenths => "charleston à la double croche",
                Hat::Quarters => "charleston à la noire",
                Hat::Open => "charleston ouvert",
                Hat::Ride => "ride",
                Hat::None => "aucune",
            },
            match self.kick {
                Kick::Accents => "sur les accents",
                Kick::FourOnFloor => "sur chaque temps",
                Kick::DoubleBass => "double pédale",
            },
            self.kick_density,
            yes_no(self.ghost_notes),
            yes_no(self.crash),
            match (self.fill, self.fill_every) {
                (false, 0) => "non".to_string(),
                (true, 0) => "à la fin".to_string(),
                (false, every) => format!("toutes les {every} mesures"),
                (true, every) => format!("toutes les {every} mesures et à la fin"),
            },
            self.intensity,
        )
    }
}

/// Ébauche de batterie calée sur les accents du morceau, avec les choix par défaut :
/// grosse caisse sur le premier temps et les accents forts, caisse claire sur 2 et 4,
/// charleston fermé à la croche, crash au début.
pub fn sketch_drums(grid: &Grid, accents: &[AccentMap]) -> Vec<DrumNote> {
    write_drums(grid, accents, &DrumPlan::default())
}

/// Écrit une partie de batterie calée sur les accents du morceau, selon les choix de `plan`.
///
/// Une mesure où rien ne sonne reste vide.
pub fn write_drums(grid: &Grid, accents: &[AccentMap], plan: &DrumPlan) -> Vec<DrumNote> {
    let plan = plan.clone().clamped();
    let combined = combined_accents(accents);
    let per_beat = grid.slots_per_beat as usize;
    let half_beat = per_beat / 2;
    // Seuil d'accent qui déclenche une grosse caisse : 0,95 (presque jamais) à densité 0,
    // 0,6 (accents forts) à 0,5, 0,25 (tous les accents) à 1.
    let kick_threshold = 0.95 - 0.7 * plan.kick_density;
    // 1 à l'intensité par défaut (0,8).
    let force = 0.6 + 0.5 * plan.intensity;
    let velocity = |base: f32| (base * force).round().clamp(1.0, 127.0) as u8;

    let on_beat = |slot: usize| slot.is_multiple_of(per_beat);
    let on_backbeat = |slot: usize| match plan.feel {
        Feel::Normal => on_beat(slot) && (slot / per_beat) % 2 == 1,
        Feel::HalfTime => on_beat(slot) && (slot / per_beat) % 4 == 2,
        Feel::DoubleTime => half_beat > 0 && slot % per_beat == half_beat,
        Feel::Blast => slot % 2 == 1,
    };
    let hat_note = match plan.hat {
        Hat::Eighths | Hat::Sixteenths | Hat::Quarters => Some(notes::HAT_CLOSED),
        Hat::Open => Some(notes::HAT_OPEN),
        Hat::Ride => Some(notes::RIDE),
        Hat::None => None,
    };
    let hat_plays = |slot: usize| match plan.hat {
        Hat::Sixteenths => true,
        Hat::Quarters => on_beat(slot),
        Hat::None => false,
        Hat::Eighths | Hat::Open | Hat::Ride => half_beat > 0 && slot.is_multiple_of(half_beat),
    };

    let sounding: Vec<usize> = combined
        .iter()
        .enumerate()
        .filter(|(_, (_, slots))| slots.iter().any(|&s| s >= ANY_ACCENT))
        .map(|(index, _)| index)
        .collect();
    let (first, last) = (sounding.first().copied(), sounding.last().copied());
    let mut part = Vec::new();

    let every = plan.fill_every as usize;
    for (position, &index) in sounding.iter().enumerate() {
        let (bar, slots) = &combined[index];
        let bar_qn = *bar as f64 * grid.beats_per_bar;
        let at = |slot: usize| bar_qn + slot as f64 / per_beat as f64;
        // Le roulement occupe le dernier temps entier de la mesure : la dernière du passage,
        // et une mesure sur `fill_every`.
        let periodic = every > 0 && (position + 1).is_multiple_of(every);
        let fill_from = (((plan.fill && Some(index) == last) || periodic)
            && slots.len() >= per_beat)
            .then(|| slots.len() - per_beat);
        // Un roulement régulier retombe sur un crash.
        let after_fill = every > 0 && position > 0 && position.is_multiple_of(every);

        for (slot, &strength) in slots.iter().enumerate() {
            if let Some(start) = fill_from.filter(|&start| slot >= start) {
                let step = slot - start;
                let fill_notes = [notes::SNARE, notes::SNARE, notes::TOM_MID, notes::TOM_FLOOR];
                part.push(DrumNote {
                    position_qn: at(slot),
                    note: fill_notes
                        [(step * fill_notes.len() / per_beat).min(fill_notes.len() - 1)],
                    velocity: velocity(84.0 + 28.0 * step as f32 / per_beat as f32),
                });
                continue;
            }
            let snare = on_backbeat(slot);
            let on_accent = slot == 0 || strength >= kick_threshold;
            let kick = if plan.feel == Feel::Blast {
                slot.is_multiple_of(2)
            } else {
                match plan.kick {
                    Kick::Accents => !snare && on_accent,
                    Kick::FourOnFloor => on_beat(slot) || (!snare && on_accent),
                    Kick::DoubleBass => true,
                }
            };
            if snare {
                part.push(DrumNote {
                    position_qn: at(slot),
                    note: notes::SNARE,
                    velocity: velocity(108.0),
                });
            }
            if kick {
                // Hors des accents, la double pédale et les temps « de remplissage » frappent
                // un peu moins fort.
                let base = if on_accent || plan.feel == Feel::Blast {
                    96.0
                } else {
                    88.0
                };
                part.push(DrumNote {
                    position_qn: at(slot),
                    note: notes::KICK,
                    velocity: velocity(base + 24.0 * strength.clamp(0.0, 1.0)),
                });
            }
            if !snare
                && !kick
                && plan.ghost_notes
                && slot + 1 < slots.len()
                && on_backbeat(slot + 1)
            {
                part.push(DrumNote {
                    position_qn: at(slot),
                    note: notes::SNARE,
                    velocity: velocity(38.0),
                });
            }
            if let Some(note) = hat_note.filter(|_| hat_plays(slot)) {
                part.push(DrumNote {
                    position_qn: at(slot),
                    note,
                    velocity: velocity(if on_beat(slot) { 88.0 } else { 66.0 }),
                });
            }
        }
        if (plan.crash && Some(index) == first) || after_fill {
            part.push(DrumNote {
                position_qn: at(0),
                note: notes::CRASH,
                velocity: velocity(104.0),
            });
        }
    }
    part
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accents::BarAccents;

    fn grid() -> Grid {
        Grid {
            bpm: 120.0,
            beats_per_bar: 4.0,
            slots_per_beat: 4,
            origin_s: 0.0,
        }
    }

    /// Une carte d'accents écrite à la main : `bars` donne les cases accentuées de chaque mesure.
    fn accents(bars: &[&[(usize, f32)]]) -> AccentMap {
        let bars: Vec<BarAccents> = bars
            .iter()
            .enumerate()
            .map(|(index, hits)| {
                let mut slots = vec![0.0; 16];
                for &(slot, strength) in *hits {
                    slots[slot] = strength;
                }
                BarAccents {
                    bar: index as i64,
                    slots,
                    energy: 1.0,
                }
            })
            .collect();
        AccentMap {
            grid: grid(),
            folded: vec![0.0; 16],
            pattern: String::new(),
            timing: None,
            onsets_used: 0,
            bars,
        }
    }

    fn hit(bar: usize, slot: usize, note: u8) -> DrumNote {
        DrumNote {
            position_qn: bar as f64 * 4.0 + slot as f64 / 4.0,
            note,
            velocity: 100,
        }
    }

    #[test]
    fn notes_map_to_the_right_piece() {
        assert_eq!(piece_of(36), Piece::Kick);
        assert_eq!(piece_of(35), Piece::Kick);
        assert_eq!(piece_of(38), Piece::Snare);
        assert_eq!(piece_of(42), Piece::HatClosed);
        assert_eq!(piece_of(63), Piece::HatClosed);
        assert_eq!(piece_of(60), Piece::HatOpen);
        assert_eq!(piece_of(49), Piece::Crash);
        assert_eq!(piece_of(31), Piece::Other);
    }

    #[test]
    fn reading_folds_each_piece_onto_the_bar() {
        // Grosse caisse sur 1 et 3 dans les deux mesures, caisse claire sur 2 et 4 dans la première.
        let part = [
            hit(0, 0, 36),
            hit(0, 8, 36),
            hit(1, 0, 36),
            hit(1, 8, 36),
            hit(0, 4, 38),
            hit(0, 12, 38),
            hit(1, 3, 31),
        ];
        let map = accents(&[&[(0, 1.0)], &[(0, 1.0)]]);
        let reading = read_drums("drums", &part, &grid(), &[map]);
        let lane = |piece| reading.lanes.iter().find(|l| l.piece == piece).unwrap();
        assert_eq!(lane(Piece::Kick).pattern, "X... .... X... ....");
        assert_eq!(lane(Piece::Kick).hits, 4);
        // Jouée dans une mesure sur deux : accent léger.
        assert_eq!(lane(Piece::Snare).pattern, ".... x... .... x...");
        assert_eq!(reading.unknown_notes, vec![(31, 1)]);
    }

    #[test]
    fn reading_measures_how_well_the_drums_follow_the_song() {
        // Le morceau accentue les cases 0 et 6 ; la batterie joue 0 (accent) et 10 (rien).
        let map = accents(&[&[(0, 1.0), (6, 0.9)]]);
        let part = [hit(0, 0, 36), hit(0, 10, 36)];
        let reading = read_drums("drums", &part, &grid(), &[map]);
        assert_eq!(reading.accents_covered, Some(0.5));
        assert_eq!(reading.kicks_on_accents, Some(0.5));
    }

    #[test]
    fn reading_without_song_accents_gives_no_score() {
        let reading = read_drums("drums", &[hit(0, 0, 36)], &grid(), &[]);
        assert_eq!(reading.accents_covered, None);
        assert_eq!(reading.kicks_on_accents, None);
        assert_eq!(reading.lanes[0].pattern, "X... .... .... ....");
    }

    #[test]
    fn sketch_puts_kicks_on_strong_accents_and_snare_on_backbeats() {
        let map = accents(&[&[(0, 1.0), (6, 0.9), (10, 0.3)]]);
        let part = sketch_drums(&grid(), &[map]);
        let slots = |note: u8| -> Vec<usize> {
            part.iter()
                .filter(|n| n.note == note)
                .map(|n| (n.position_qn * 4.0).round() as usize)
                .collect()
        };
        // L'accent léger de la case 10 ne déclenche pas de grosse caisse.
        assert_eq!(slots(notes::KICK), vec![0, 6]);
        assert_eq!(slots(notes::SNARE), vec![4, 12]);
        assert_eq!(slots(notes::HAT_CLOSED), vec![0, 2, 4, 6, 8, 10, 12, 14]);
        assert_eq!(slots(notes::CRASH), vec![0]);
    }

    #[test]
    fn sketch_keeps_the_snare_when_an_accent_falls_on_a_backbeat() {
        let map = accents(&[&[(0, 1.0), (4, 1.0)]]);
        let part = sketch_drums(&grid(), &[map]);
        assert!(!part
            .iter()
            .any(|n| n.note == notes::KICK && (n.position_qn - 1.0).abs() < 1e-9));
    }

    #[test]
    fn sketch_leaves_silent_bars_empty_and_crashes_once() {
        let map = accents(&[&[(0, 1.0)], &[], &[(0, 1.0)]]);
        let part = sketch_drums(&grid(), &[map]);
        assert!(!part.iter().any(|n| (4.0..8.0).contains(&n.position_qn)));
        assert_eq!(part.iter().filter(|n| n.note == notes::CRASH).count(), 1);
    }

    fn slots_of(part: &[DrumNote], note: u8) -> Vec<usize> {
        part.iter()
            .filter(|n| n.note == note)
            .map(|n| (n.position_qn * 4.0).round() as usize)
            .collect()
    }

    #[test]
    fn half_time_moves_the_snare_to_beat_three() {
        let map = accents(&[&[(0, 1.0)]]);
        let plan = DrumPlan {
            feel: Feel::HalfTime,
            ..DrumPlan::default()
        };
        assert_eq!(
            slots_of(&write_drums(&grid(), &[map], &plan), notes::SNARE),
            vec![8]
        );
    }

    #[test]
    fn double_time_puts_the_snare_on_every_offbeat() {
        let map = accents(&[&[(0, 1.0)]]);
        let plan = DrumPlan {
            feel: Feel::DoubleTime,
            ..DrumPlan::default()
        };
        assert_eq!(
            slots_of(&write_drums(&grid(), &[map], &plan), notes::SNARE),
            vec![2, 6, 10, 14]
        );
    }

    #[test]
    fn kick_density_decides_which_accents_get_a_kick() {
        let map = accents(&[&[(0, 1.0), (6, 0.9), (10, 0.3)]]);
        let kicks = |density: f32| {
            let plan = DrumPlan {
                kick_density: density,
                ..DrumPlan::default()
            };
            slots_of(
                &write_drums(&grid(), std::slice::from_ref(&map), &plan),
                notes::KICK,
            )
        };
        assert_eq!(kicks(0.0), vec![0]);
        assert_eq!(kicks(0.5), vec![0, 6]);
        assert_eq!(kicks(1.0), vec![0, 6, 10]);
    }

    #[test]
    fn hat_choice_changes_the_note_and_the_rate() {
        let map = accents(&[&[(0, 1.0)]]);
        let with = |hat: Hat| {
            let plan = DrumPlan {
                hat,
                ..DrumPlan::default()
            };
            write_drums(&grid(), std::slice::from_ref(&map), &plan)
        };
        assert_eq!(
            slots_of(&with(Hat::Sixteenths), notes::HAT_CLOSED).len(),
            16
        );
        assert_eq!(
            slots_of(&with(Hat::Quarters), notes::HAT_CLOSED),
            vec![0, 4, 8, 12]
        );
        assert_eq!(slots_of(&with(Hat::Ride), notes::RIDE).len(), 8);
        assert_eq!(slots_of(&with(Hat::Open), notes::HAT_OPEN).len(), 8);
        let none = with(Hat::None);
        assert!(!none.iter().any(|n| matches!(
            piece_of(n.note),
            Piece::HatClosed | Piece::HatOpen | Piece::Ride
        )));
    }

    #[test]
    fn ghost_notes_sit_just_before_the_snare_and_stay_quiet() {
        let map = accents(&[&[(0, 1.0)]]);
        let plan = DrumPlan {
            ghost_notes: true,
            ..DrumPlan::default()
        };
        let part = write_drums(&grid(), &[map], &plan);
        let snares: Vec<(usize, u8)> = part
            .iter()
            .filter(|n| n.note == notes::SNARE)
            .map(|n| ((n.position_qn * 4.0).round() as usize, n.velocity))
            .collect();
        assert_eq!(
            snares.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec![3, 4, 11, 12]
        );
        assert!(snares[0].1 < 50 && snares[1].1 > 100, "{snares:?}");
    }

    #[test]
    fn fill_replaces_the_last_beat_of_the_last_bar_only() {
        let map = accents(&[&[(0, 1.0)], &[(0, 1.0)]]);
        let plan = DrumPlan {
            fill: true,
            ..DrumPlan::default()
        };
        let part = write_drums(&grid(), &[map], &plan);
        let last_beat: Vec<u8> = part
            .iter()
            .filter(|n| n.position_qn >= 7.0)
            .map(|n| n.note)
            .collect();
        assert_eq!(
            last_beat,
            vec![notes::SNARE, notes::SNARE, notes::TOM_MID, notes::TOM_FLOOR]
        );
        // La première mesure garde sa caisse claire sur le temps 4 et son charleston.
        assert!(part
            .iter()
            .any(|n| n.note == notes::SNARE && (n.position_qn - 3.0).abs() < 1e-9));
        assert!(part
            .iter()
            .any(|n| n.note == notes::HAT_CLOSED && (n.position_qn - 3.5).abs() < 1e-9));
    }

    #[test]
    fn intensity_scales_how_hard_the_drums_hit() {
        let map = accents(&[&[(0, 1.0)]]);
        let snare = |intensity: f32| {
            let plan = DrumPlan {
                intensity,
                ..DrumPlan::default()
            };
            write_drums(&grid(), std::slice::from_ref(&map), &plan)
                .iter()
                .find(|n| n.note == notes::SNARE)
                .unwrap()
                .velocity
        };
        assert!(snare(0.2) < snare(0.8) && snare(0.8) < snare(1.0));
        assert_eq!(snare(0.8), 108);
    }

    #[test]
    fn out_of_range_choices_are_brought_back_in_range() {
        let plan = DrumPlan {
            kick_density: 3.0,
            intensity: f32::NAN,
            ..DrumPlan::default()
        }
        .clamped();
        assert_eq!(plan.kick_density, 1.0);
        assert_eq!(plan.intensity, 0.5);
    }

    #[test]
    fn from_scratch_the_plan_alone_shapes_the_groove() {
        use crate::accents::template_accents;
        // Deux mesures vides à 120 BPM.
        let template = template_accents(grid(), 4.0);
        let kicks = |density: f32| {
            let plan = DrumPlan {
                kick_density: density,
                ..DrumPlan::default()
            };
            let part = write_drums(&grid(), std::slice::from_ref(&template), &plan);
            part.iter()
                .filter(|n| n.note == notes::KICK && n.position_qn < 4.0)
                .map(|n| (n.position_qn * 4.0).round() as usize)
                .collect::<Vec<_>>()
        };
        // Grosse caisse sur 1 seulement, puis 1 et 3, puis avec les contretemps.
        assert_eq!(kicks(0.0), vec![0]);
        assert_eq!(kicks(0.5), vec![0, 8]);
        assert_eq!(kicks(1.0), vec![0, 6, 7, 8, 14]);
        // La batterie complète est là : caisse claire sur 2 et 4, charleston à la croche.
        let part = write_drums(&grid(), &[template], &DrumPlan::default());
        assert_eq!(slots_of(&part, notes::SNARE), vec![4, 12, 20, 28]);
        assert_eq!(slots_of(&part, notes::HAT_CLOSED).len(), 16);
    }

    #[test]
    fn double_bass_plays_every_slot_and_keeps_the_snare() {
        let map = accents(&[&[(0, 1.0)]]);
        let plan = DrumPlan {
            kick: Kick::DoubleBass,
            ..DrumPlan::default()
        };
        let part = write_drums(&grid(), &[map], &plan);
        assert_eq!(slots_of(&part, notes::KICK).len(), 16);
        assert_eq!(slots_of(&part, notes::SNARE), vec![4, 12]);
    }

    #[test]
    fn four_on_the_floor_kicks_every_beat_even_under_the_snare() {
        let map = accents(&[&[(0, 1.0), (6, 0.9)]]);
        let plan = DrumPlan {
            kick: Kick::FourOnFloor,
            ..DrumPlan::default()
        };
        let part = write_drums(&grid(), &[map], &plan);
        // Les quatre temps, plus l'accent fort de la case 6.
        assert_eq!(slots_of(&part, notes::KICK), vec![0, 4, 6, 8, 12]);
        assert_eq!(slots_of(&part, notes::SNARE), vec![4, 12]);
    }

    #[test]
    fn blast_alternates_kick_and_snare() {
        let map = accents(&[&[(0, 1.0)]]);
        let plan = DrumPlan {
            feel: Feel::Blast,
            ..DrumPlan::default()
        };
        let part = write_drums(&grid(), &[map], &plan);
        assert_eq!(
            slots_of(&part, notes::KICK),
            vec![0, 2, 4, 6, 8, 10, 12, 14]
        );
        assert_eq!(
            slots_of(&part, notes::SNARE),
            vec![1, 3, 5, 7, 9, 11, 13, 15]
        );
    }

    #[test]
    fn regular_fills_come_every_n_bars_and_land_on_a_crash() {
        let map = accents(&[&[(0, 1.0)], &[(0, 1.0)], &[(0, 1.0)], &[(0, 1.0)]]);
        let plan = DrumPlan {
            fill_every: 2,
            ..DrumPlan::default()
        };
        let part = write_drums(&grid(), &[map], &plan);
        let toms: Vec<usize> = part
            .iter()
            .filter(|n| piece_of(n.note) == Piece::Tom)
            .map(|n| (n.position_qn / 4.0) as usize)
            .collect();
        // Roulements à la fin des mesures 2 et 4 (numéros 1 et 3).
        assert_eq!(toms, vec![1, 1, 3, 3]);
        // Crash au début, puis au retour du premier roulement.
        assert_eq!(slots_of(&part, notes::CRASH), vec![0, 32]);
    }

    #[test]
    fn summary_names_the_choices() {
        let plan = DrumPlan {
            feel: Feel::Blast,
            kick: Kick::DoubleBass,
            fill_every: 4,
            ..DrumPlan::default()
        };
        let summary = plan.summary();
        assert!(summary.contains("blast beat"), "{summary}");
        assert!(summary.contains("double pédale"), "{summary}");
        assert!(summary.contains("toutes les 4 mesures"), "{summary}");
    }

    #[test]
    fn sketch_of_nothing_is_empty() {
        assert!(sketch_drums(&grid(), &[]).is_empty());
    }
}
