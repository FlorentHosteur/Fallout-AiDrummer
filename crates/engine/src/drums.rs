//! Batterie : table de notes, lecture d'une partie existante, ébauche d'une partie nouvelle.
//!
//! Les positions sont en noires, comptées depuis le début de la mesure 0 de la grille
//! (celle qui contient le début du passage).

use serde::Serialize;

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
    pub const CRASH: u8 = 49;
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
const STRONG_ACCENT: f32 = 0.6;
const ANY_ACCENT: f32 = 0.25;

/// Case de la grille (numéro de mesure, case dans la mesure) la plus proche d'une position.
fn slot_of(position_qn: f64, grid: &Grid) -> (i64, usize) {
    let per_bar = grid.slots_per_bar() as i64;
    let global = (position_qn * grid.slots_per_beat as f64).round() as i64;
    (global.div_euclid(per_bar), global.rem_euclid(per_bar) as usize)
}

/// Force de l'accent le plus fort du morceau dans chaque case de chaque mesure,
/// toutes pistes confondues.
fn combined_accents(tracks: &[AccentMap]) -> Vec<(i64, Vec<f32>)> {
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
pub fn read_drums(track: &str, part: &[DrumNote], grid: &Grid, accents: &[AccentMap]) -> DrumReading {
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
        kicks.iter().filter(|cell| accent_at(cell) >= ANY_ACCENT).count() as f32
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

/// Ébauche de batterie calée sur les accents du morceau.
///
/// Règles volontairement simples, pour avoir un point de départ à retoucher :
/// - grosse caisse sur le premier temps et sur chaque accent fort du morceau ;
/// - caisse claire sur les temps 2 et 4 (un temps sur deux) ;
/// - charleston fermé à la croche, plus appuyé sur les temps ;
/// - crash sur le premier temps de la première mesure.
///
/// Une mesure où rien ne sonne reste vide.
pub fn sketch_drums(grid: &Grid, accents: &[AccentMap]) -> Vec<DrumNote> {
    let combined = combined_accents(accents);
    let slots_per_beat = grid.slots_per_beat as usize;
    let mut part = Vec::new();

    for (index, (bar, slots)) in combined.iter().enumerate() {
        if slots.iter().all(|&s| s < ANY_ACCENT) {
            continue;
        }
        let bar_qn = *bar as f64 * grid.beats_per_bar;
        let at = |slot: usize| bar_qn + slot as f64 / slots_per_beat as f64;
        let on_backbeat =
            |slot: usize| slot.is_multiple_of(slots_per_beat) && (slot / slots_per_beat) % 2 == 1;

        for (slot, &strength) in slots.iter().enumerate() {
            if on_backbeat(slot) {
                part.push(DrumNote {
                    position_qn: at(slot),
                    note: notes::SNARE,
                    velocity: 108,
                });
            } else if slot == 0 || strength >= STRONG_ACCENT {
                part.push(DrumNote {
                    position_qn: at(slot),
                    note: notes::KICK,
                    velocity: (96.0 + 24.0 * strength.clamp(0.0, 1.0)) as u8,
                });
            }
            // Croches : une case sur deux quand la grille est en doubles croches.
            let eighth = slots_per_beat.is_multiple_of(2) && slot % (slots_per_beat / 2) == 0;
            if eighth {
                part.push(DrumNote {
                    position_qn: at(slot),
                    note: notes::HAT_CLOSED,
                    velocity: if slot % slots_per_beat == 0 { 88 } else { 66 },
                });
            }
        }
        if index == 0 {
            part.push(DrumNote {
                position_qn: at(0),
                note: notes::CRASH,
                velocity: 104,
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

    #[test]
    fn sketch_of_nothing_is_empty() {
        assert!(sketch_drums(&grid(), &[]).is_empty());
    }
}
