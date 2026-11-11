//! Conseils sur une partie de batterie existante.
//!
//! Chaque conseil vient d'une comparaison entre la batterie et les accents du morceau, et
//! porte les retouches qui le réalisent. Les retouches ne s'appliquent jamais à la partie
//! d'origine : `apply_advice` en rend une copie corrigée.

use serde::Serialize;

use crate::accents::{AccentMap, Grid};
use crate::drums::{
    combined_accents, notes, piece_of, slot_of, DrumNote, Piece, ANY_ACCENT, STRONG_ACCENT,
};

/// Une retouche d'une partie de batterie.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Edit {
    Add(DrumNote),
    /// Retire le coup d'origine qui a cette position et cette note.
    Remove {
        position_qn: f64,
        note: u8,
    },
    /// Change la vélocité du coup d'origine qui a cette position et cette note.
    SetVelocity {
        position_qn: f64,
        note: u8,
        velocity: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Advice {
    /// Le conseil en une ligne.
    pub title: String,
    /// Où il s'applique, et pourquoi.
    pub detail: String,
    pub edits: Vec<Edit>,
}

/// Part des coups de grosse caisse posés sur un accent à partir de laquelle on considère que
/// la grosse caisse suit le morceau.
const FOLLOWS_SHARE: f32 = 0.5;
/// Nombre d'endroits cités dans le détail d'un conseil ; les suivants sont comptés.
const PLACES_SHOWN: usize = 6;
/// En dessous de cet écart entre le coup le plus fort et le plus faible, un jeu est dit plat.
const FLAT_VELOCITY_RANGE: u8 = 4;
/// Nombre de coups à partir duquel on juge la dynamique d'un élément.
const ENOUGH_HITS: usize = 8;
/// Force d'un contretemps par rapport à un temps, dans la dynamique proposée.
const OFFBEAT_FACTOR: f32 = 0.75;

/// Compare la batterie aux accents du morceau et propose des retouches.
///
/// `accents` : les cartes d'accents des pistes audio du même passage, sur la même grille.
/// Sans accents, seuls les conseils sur la dynamique sont possibles.
pub fn advise(part: &[DrumNote], grid: &Grid, accents: &[AccentMap]) -> Vec<Advice> {
    advise_at(part, grid, accents, 1)
}

/// Comme `advise`, en nommant les endroits par leur numéro de mesure dans le projet.
/// `first_measure` : numéro de la mesure 0 de la grille.
pub fn advise_at(
    part: &[DrumNote],
    grid: &Grid,
    accents: &[AccentMap],
    first_measure: i64,
) -> Vec<Advice> {
    let combined = combined_accents(accents);
    let per_beat = grid.slots_per_beat as usize;
    let accent_at = |cell: (i64, usize)| -> f32 {
        combined
            .iter()
            .find(|(bar, _)| *bar == cell.0)
            .and_then(|(_, slots)| slots.get(cell.1))
            .copied()
            .unwrap_or(0.0)
    };
    // Avec plusieurs pistes, un accent n'appelle la batterie que si au moins deux le jouent :
    // un coup isolé d'une seule piste est un détail de son jeu, pas un appui du morceau.
    let tracks_needed = if accents.len() > 1 { 2 } else { 1 };
    let shared = |cell: (i64, usize)| -> bool {
        accents
            .iter()
            .filter(|track| {
                track
                    .bars
                    .iter()
                    .find(|bar| bar.bar == cell.0)
                    .and_then(|bar| bar.slots.get(cell.1))
                    .is_some_and(|&strength| strength >= ANY_ACCENT)
            })
            .count()
            >= tracks_needed
    };
    let place = |cell: (i64, usize)| place_name(first_measure - 1 + cell.0, cell.1, per_beat);
    let position_of =
        |cell: (i64, usize)| cell.0 as f64 * grid.beats_per_bar + cell.1 as f64 / per_beat as f64;

    let backbone: Vec<(i64, usize)> = part
        .iter()
        .filter(|n| matches!(piece_of(n.note), Piece::Kick | Piece::Snare))
        .map(|n| slot_of(n.position_qn, grid))
        .collect();
    let kicks: Vec<&DrumNote> = part
        .iter()
        .filter(|n| piece_of(n.note) == Piece::Kick)
        .collect();

    let mut advices = Vec::new();
    let mut moved: Vec<f64> = Vec::new();
    let (mut shift_places, mut shift_edits) = (Vec::new(), Vec::new());
    let (mut add_places, mut add_edits) = (Vec::new(), Vec::new());

    for (bar, slots) in &combined {
        for (slot, &strength) in slots.iter().enumerate() {
            let cell = (*bar, slot);
            if strength < STRONG_ACCENT || backbone.contains(&cell) || !shared(cell) {
                continue;
            }
            let kick = DrumNote {
                position_qn: position_of(cell),
                note: notes::KICK,
                velocity: (96.0 + 24.0 * strength.clamp(0.0, 1.0)) as u8,
            };
            // Une grosse caisse posée juste à côté, sur une case où le morceau ne joue rien :
            // elle vise sans doute cet accent.
            let neighbour = kicks.iter().find(|k| {
                let other = slot_of(k.position_qn, grid);
                other.0 == *bar
                    && other.1.abs_diff(slot) == 1
                    && accent_at(other) < ANY_ACCENT
                    && !moved.contains(&k.position_qn)
            });
            match neighbour {
                Some(near) => {
                    moved.push(near.position_qn);
                    shift_places.push(format!(
                        "{} → {}",
                        place(slot_of(near.position_qn, grid)),
                        place(cell)
                    ));
                    shift_edits.push(Edit::Remove {
                        position_qn: near.position_qn,
                        note: near.note,
                    });
                    shift_edits.push(Edit::Add(DrumNote {
                        velocity: near.velocity,
                        ..kick
                    }));
                }
                None => {
                    add_places.push(place(cell));
                    add_edits.push(Edit::Add(kick));
                }
            }
        }
    }
    if !shift_edits.is_empty() {
        advices.push(Advice {
            title: format!(
                "Recaler {} sur l'accent voisin du morceau",
                count(shift_places.len(), "grosse caisse", "grosses caisses")
            ),
            detail: format!(
                "Ces coups tombent une double croche à côté d'un accent fort : {}.",
                list(&shift_places)
            ),
            edits: shift_edits,
        });
    }
    if !add_edits.is_empty() {
        advices.push(Advice {
            title: format!(
                "Appuyer {} du morceau par une grosse caisse",
                count(add_places.len(), "accent fort", "accents forts")
            ),
            detail: format!(
                "{} ces endroits, la batterie n'y joue ni grosse caisse ni caisse claire : {}.",
                if accents.len() > 1 {
                    "Plusieurs pistes accentuent ensemble"
                } else {
                    "Le morceau accentue"
                },
                list(&add_places)
            ),
            edits: add_edits,
        });
    }

    // Grosses caisses en l'air : hors des temps, là où le morceau ne joue rien. On ne le dit que
    // si la grosse caisse suit le morceau dans l'ensemble : quand la plupart de ses coups tombent
    // ailleurs que sur les accents (double pédale, passage où peu de pistes sonnent), elle a sa
    // propre logique et ces coups ne sont pas des écarts.
    let on_accents = kicks
        .iter()
        .filter(|k| accent_at(slot_of(k.position_qn, grid)) >= ANY_ACCENT)
        .count();
    let follows_the_song = on_accents as f32 >= FOLLOWS_SHARE * kicks.len() as f32;
    if !combined.is_empty() && follows_the_song {
        let stray: Vec<&&DrumNote> = kicks
            .iter()
            .filter(|k| {
                let cell = slot_of(k.position_qn, grid);
                !cell.1.is_multiple_of(per_beat)
                    && accent_at(cell) < ANY_ACCENT
                    && !moved.contains(&k.position_qn)
            })
            .collect();
        if !stray.is_empty() {
            let places: Vec<String> = stray
                .iter()
                .map(|k| place(slot_of(k.position_qn, grid)))
                .collect();
            advices.push(Advice {
                title: format!(
                    "Alléger {} dans le morceau",
                    count(
                        stray.len(),
                        "grosse caisse qui ne répond à rien",
                        "grosses caisses qui ne répondent à rien"
                    )
                ),
                detail: format!(
                    "Ces coups tombent entre les temps, là où aucune piste n'attaque : {}.",
                    list(&places)
                ),
                edits: stray
                    .iter()
                    .map(|k| Edit::Remove {
                        position_qn: k.position_qn,
                        note: k.note,
                    })
                    .collect(),
            });
        }
    }

    // Main droite sans relief.
    for (piece, name) in [(Piece::HatClosed, "charleston"), (Piece::Ride, "ride")] {
        let hits: Vec<&DrumNote> = part.iter().filter(|n| piece_of(n.note) == piece).collect();
        let (Some(softest), Some(loudest)) = (
            hits.iter().map(|n| n.velocity).min(),
            hits.iter().map(|n| n.velocity).max(),
        ) else {
            continue;
        };
        let off_beats: Vec<&&DrumNote> = hits
            .iter()
            .filter(|n| !slot_of(n.position_qn, grid).1.is_multiple_of(per_beat))
            .collect();
        if hits.len() < ENOUGH_HITS
            || loudest - softest > FLAT_VELOCITY_RANGE
            || off_beats.is_empty()
            || off_beats.len() == hits.len()
        {
            continue;
        }
        advices.push(Advice {
            title: format!("Donner du relief au jeu de {name}"),
            detail: format!(
                "Les {} coups ont presque tous la même force ({softest} à {loudest}) : un batteur appuie les temps et allège les contretemps.",
                hits.len()
            ),
            edits: off_beats
                .iter()
                .map(|n| Edit::SetVelocity {
                    position_qn: n.position_qn,
                    note: n.note,
                    velocity: ((n.velocity as f32 * OFFBEAT_FACTOR).round() as u8).max(1),
                })
                .collect(),
        });
    }

    advices
}

/// Rend une copie de `part` avec les retouches des conseils retenus.
pub fn apply_advice(part: &[DrumNote], advices: &[&Advice]) -> Vec<DrumNote> {
    let same = |n: &DrumNote, position_qn: f64, note: u8| {
        n.note == note && (n.position_qn - position_qn).abs() < 1e-9
    };
    let mut result = part.to_vec();
    for edit in advices.iter().flat_map(|advice| &advice.edits) {
        match *edit {
            Edit::Add(note) => {
                if !result.iter().any(|n| same(n, note.position_qn, note.note)) {
                    result.push(note);
                }
            }
            Edit::Remove { position_qn, note } => result.retain(|n| !same(n, position_qn, note)),
            Edit::SetVelocity {
                position_qn,
                note,
                velocity,
            } => {
                for n in result.iter_mut().filter(|n| same(n, position_qn, note)) {
                    n.velocity = velocity;
                }
            }
        }
    }
    result.sort_by(|a, b| {
        a.position_qn
            .total_cmp(&b.position_qn)
            .then(a.note.cmp(&b.note))
    });
    result
}

/// Les conseils en texte, numérotés, pour la console ou le journal.
pub fn advice_report(track: &str, advices: &[Advice]) -> String {
    if advices.is_empty() {
        return format!(
            "Batterie « {track} » : rien à signaler. Elle suit les accents du morceau et son jeu a du relief.\n"
        );
    }
    let mut text = format!(
        "Batterie « {track} » : {}\n",
        count(advices.len(), "conseil", "conseils")
    );
    for (index, advice) in advices.iter().enumerate() {
        text.push_str(&format!(
            "  {}. {}\n     {}\n",
            index + 1,
            advice.title,
            advice.detail
        ));
    }
    text
}

fn count(number: usize, singular: &str, plural: &str) -> String {
    format!("{number} {}", if number > 1 { plural } else { singular })
}

fn list(places: &[String]) -> String {
    let shown = places
        .iter()
        .take(PLACES_SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ; ");
    match places.len().saturating_sub(PLACES_SHOWN) {
        0 => shown,
        more => format!("{shown} ; et {more} de plus"),
    }
}

/// Nom d'une case : « mesure 2, temps 3 », « mesure 2, contretemps du temps 3 »,
/// « mesure 2, 2e double croche du temps 3 ». `bar` compte depuis 0, la première mesure du passage.
fn place_name(bar: i64, slot: usize, per_beat: usize) -> String {
    let beat = slot / per_beat + 1;
    let within = slot % per_beat;
    let position = if within == 0 {
        format!("temps {beat}")
    } else if per_beat.is_multiple_of(2) && within == per_beat / 2 {
        format!("contretemps du temps {beat}")
    } else {
        format!(
            "{}e {} du temps {beat}",
            within + 1,
            if per_beat == 4 {
                "double croche"
            } else {
                "division"
            }
        )
    };
    format!("mesure {}, {position}", bar + 1)
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

    fn accents(bars: &[&[(usize, f32)]]) -> AccentMap {
        AccentMap {
            grid: grid(),
            folded: vec![0.0; 16],
            pattern: String::new(),
            timing: None,
            onsets_used: 0,
            bars: bars
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
                .collect(),
        }
    }

    fn hit(bar: usize, slot: usize, note: u8, velocity: u8) -> DrumNote {
        DrumNote {
            position_qn: bar as f64 * 4.0 + slot as f64 / 4.0,
            note,
            velocity,
        }
    }

    fn slots_of(part: &[DrumNote], note: u8) -> Vec<usize> {
        part.iter()
            .filter(|n| n.note == note)
            .map(|n| (n.position_qn * 4.0).round() as usize)
            .collect()
    }

    #[test]
    fn a_drum_part_that_follows_the_song_gets_no_advice() {
        let map = accents(&[&[(0, 1.0), (6, 0.9)]]);
        let part = [hit(0, 0, 36, 110), hit(0, 6, 36, 100), hit(0, 4, 38, 105)];
        assert!(advise(&part, &grid(), &[map]).is_empty());
    }

    #[test]
    fn a_strong_accent_with_no_kick_or_snare_gets_a_kick() {
        let map = accents(&[&[(0, 1.0), (10, 0.9)]]);
        let part = [hit(0, 0, 36, 110)];
        let advices = advise(&part, &grid(), &[map]);
        assert_eq!(advices.len(), 1);
        assert!(
            advices[0].title.contains("1 accent fort"),
            "{}",
            advices[0].title
        );
        assert!(
            advices[0]
                .detail
                .contains("mesure 1, contretemps du temps 3"),
            "{}",
            advices[0].detail
        );
        let fixed = apply_advice(&part, &[&advices[0]]);
        assert_eq!(slots_of(&fixed, 36), vec![0, 10]);
    }

    #[test]
    fn with_several_tracks_only_shared_accents_call_for_a_kick() {
        // La case 10 est jouée par les deux pistes, la case 6 par une seule.
        let guitar = accents(&[&[(0, 1.0), (6, 0.9), (10, 0.9)]]);
        let bass = accents(&[&[(0, 1.0), (10, 0.4)]]);
        let part = [hit(0, 0, 36, 110)];
        let advices = advise(&part, &grid(), &[guitar, bass]);
        assert_eq!(advices.len(), 1, "{advices:?}");
        assert!(advices[0]
            .detail
            .starts_with("Plusieurs pistes accentuent ensemble"));
        assert_eq!(
            slots_of(&apply_advice(&part, &[&advices[0]]), 36),
            vec![0, 10]
        );
    }

    #[test]
    fn places_use_the_project_measure_numbers() {
        let map = accents(&[&[(0, 1.0), (10, 0.9)]]);
        let advices = advise_at(&[hit(0, 0, 36, 110)], &grid(), &[map], 136);
        assert!(
            advices[0]
                .detail
                .contains("mesure 136, contretemps du temps 3"),
            "{}",
            advices[0].detail
        );
    }

    #[test]
    fn a_kick_one_slot_off_an_accent_is_moved_onto_it() {
        // Accent fort sur la case 6, grosse caisse posée sur la case 7 où rien ne joue.
        let map = accents(&[&[(0, 1.0), (6, 0.9)]]);
        let part = [hit(0, 0, 36, 110), hit(0, 7, 36, 93)];
        let advices = advise(&part, &grid(), &[map]);
        assert_eq!(advices.len(), 1, "{advices:?}");
        assert!(
            advices[0].title.starts_with("Recaler 1 grosse caisse"),
            "{}",
            advices[0].title
        );
        let fixed = apply_advice(&part, &[&advices[0]]);
        assert_eq!(slots_of(&fixed, 36), vec![0, 6]);
        // Le coup déplacé garde sa force.
        assert_eq!(
            fixed
                .iter()
                .find(|n| n.position_qn == 1.5)
                .unwrap()
                .velocity,
            93
        );
    }

    #[test]
    fn a_kick_between_beats_where_nothing_plays_is_lightened() {
        let map = accents(&[&[(0, 1.0), (4, 0.5)]]);
        // Cases 0 et 4 : sur des accents. Case 8 : sur un temps, on la garde même sans accent.
        // Case 11 : en l'air.
        let part = [
            hit(0, 0, 36, 110),
            hit(0, 4, 36, 100),
            hit(0, 8, 36, 100),
            hit(0, 11, 36, 90),
        ];
        let advices = advise(&part, &grid(), &[map]);
        assert_eq!(advices.len(), 1, "{advices:?}");
        assert!(advices[0].title.starts_with("Alléger 1 grosse caisse"));
        assert_eq!(
            slots_of(&apply_advice(&part, &[&advices[0]]), 36),
            vec![0, 4, 8]
        );
    }

    #[test]
    fn a_kick_part_with_its_own_logic_is_not_told_to_lighten() {
        // Double pédale sur un morceau qui n'attaque qu'au premier temps : une seule grosse
        // caisse sur huit tombe sur un accent. Ce n'est pas un écart, c'est un parti pris.
        let map = accents(&[&[(0, 1.0)]]);
        let part: Vec<DrumNote> = (0..8).map(|i| hit(0, i * 2 + 1, 36, 100)).collect();
        let mut part = part;
        part.push(hit(0, 0, 36, 110));
        assert!(advise(&part, &grid(), &[map]).is_empty());
    }

    #[test]
    fn a_flat_hat_gets_softer_offbeats() {
        let map = accents(&[&[(0, 1.0)]]);
        let mut part = vec![hit(0, 0, 36, 110)];
        part.extend((0..8).map(|i| hit(0, i * 2, 42, 100)));
        let advices = advise(&part, &grid(), &[map]);
        assert_eq!(advices.len(), 1, "{advices:?}");
        assert!(advices[0].title.contains("charleston"));
        let fixed = apply_advice(&part, &[&advices[0]]);
        let hats: Vec<u8> = fixed
            .iter()
            .filter(|n| n.note == 42)
            .map(|n| n.velocity)
            .collect();
        assert_eq!(hats, vec![100, 75, 100, 75, 100, 75, 100, 75]);
    }

    #[test]
    fn a_hat_that_already_has_dynamics_is_left_alone() {
        let map = accents(&[&[(0, 1.0)]]);
        let mut part = vec![hit(0, 0, 36, 110)];
        part.extend((0..8).map(|i| hit(0, i * 2, 42, if i % 2 == 0 { 100 } else { 70 })));
        assert!(advise(&part, &grid(), &[map]).is_empty());
    }

    #[test]
    fn only_the_chosen_advice_is_applied_and_the_original_is_untouched() {
        let map = accents(&[&[(0, 1.0), (10, 0.9)]]);
        let mut part = vec![hit(0, 0, 36, 110), hit(0, 13, 36, 90)];
        part.extend((0..8).map(|i| hit(0, i * 2, 42, 100)));
        let original = part.clone();
        let advices = advise(&part, &grid(), &[map]);
        assert_eq!(advices.len(), 3, "{advices:?}");
        let fixed = apply_advice(&part, &[&advices[0]]);
        // Seul l'accent manquant est ajouté ; la grosse caisse en l'air et le charleston restent.
        assert_eq!(slots_of(&fixed, 36), vec![0, 10, 13]);
        assert!(fixed
            .iter()
            .filter(|n| n.note == 42)
            .all(|n| n.velocity == 100));
        assert_eq!(part, original);
    }

    #[test]
    fn many_places_are_counted_rather_than_all_listed() {
        let bars: Vec<Vec<(usize, f32)>> = (0..8).map(|_| vec![(0, 1.0), (6, 0.9)]).collect();
        let bar_refs: Vec<&[(usize, f32)]> = bars.iter().map(Vec::as_slice).collect();
        let map = accents(&bar_refs);
        let part: Vec<DrumNote> = (0..8).map(|bar| hit(bar, 0, 36, 110)).collect();
        let advices = advise(&part, &grid(), &[map]);
        assert!(advices[0].title.contains("8 accents forts"));
        assert!(
            advices[0].detail.contains("et 2 de plus"),
            "{}",
            advices[0].detail
        );
    }

    #[test]
    fn report_numbers_the_advice_or_says_all_is_well() {
        let map = accents(&[&[(0, 1.0), (10, 0.9)]]);
        let advices = advise(&[hit(0, 0, 36, 110)], &grid(), &[map]);
        let text = advice_report("drums", &advices);
        assert!(
            text.starts_with("Batterie « drums » : 1 conseil\n  1. Appuyer"),
            "{text}"
        );
        assert!(advice_report("drums", &[]).contains("rien à signaler"));
    }

    #[test]
    fn places_are_named_the_way_a_musician_says_them() {
        assert_eq!(place_name(0, 0, 4), "mesure 1, temps 1");
        assert_eq!(place_name(1, 10, 4), "mesure 2, contretemps du temps 3");
        assert_eq!(place_name(0, 5, 3), "mesure 1, 3e division du temps 2");
        assert_eq!(place_name(0, 5, 4), "mesure 1, 2e double croche du temps 2");
        assert_eq!(place_name(0, 7, 4), "mesure 1, 4e double croche du temps 2");
    }
}
