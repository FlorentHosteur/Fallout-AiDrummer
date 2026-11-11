//! Vérifie la détection d'attaques et la carte d'accents sur un riff de synthèse dont on
//! connaît chaque note. Ne dit rien de la justesse sur un vrai instrument enregistré.

use aidrummer_engine::{accent_map, detect_onsets, detect_onsets_after_preroll, Audio, Grid};

const SAMPLE_RATE: u32 = 44_100;
const BPM: f64 = 100.0;
const BARS: usize = 4;
/// Le riff d'une mesure : (case en doubles croches, force de la note).
const RIFF: [(usize, f64); 6] = [(0, 1.0), (3, 0.5), (6, 0.8), (8, 1.0), (10, 0.5), (12, 0.9)];
/// La hauteur de chaque note du riff, en hertz (la, la, do, ré, la, mi).
const PITCHES: [f64; 6] = [110.0, 110.0, 130.81, 146.83, 110.0, 164.81];

fn grid() -> Grid {
    Grid {
        bpm: BPM,
        beats_per_bar: 4.0,
        slots_per_beat: 4,
        origin_s: 0.0,
    }
}

/// Notes pincées façon basse (fondamentale + harmoniques, qui s'éteignent en résonnant),
/// jouées avec `delay_s` de retard sur la grille.
fn riff(delay_s: f64) -> Audio {
    let slot_s = grid().slot_s();
    let bar_s = grid().bar_s();
    let total = ((BARS as f64 * bar_s + 0.5) * SAMPLE_RATE as f64) as usize;
    let mut samples = vec![0.0f32; total];
    for bar in 0..BARS {
        for ((slot, gain), pitch) in RIFF.into_iter().zip(PITCHES) {
            let start_s = bar as f64 * bar_s + slot as f64 * slot_s + delay_s;
            let start = (start_s * SAMPLE_RATE as f64).round() as usize;
            for (i, sample) in samples.iter_mut().enumerate().skip(start) {
                let t = (i - start) as f64 / SAMPLE_RATE as f64;
                if t > 1.2 {
                    break;
                }
                let attack = (t / 0.003).min(1.0);
                let tone: f64 = [(1.0, 1.0), (2.0, 0.5), (3.0, 0.3), (8.0, 0.2)]
                    .iter()
                    .map(|(harmonic, a)| a * (std::f64::consts::TAU * pitch * harmonic * t).sin())
                    .sum();
                *sample += (0.25 * gain * attack * (-t * 6.0).exp() * tone) as f32;
            }
        }
    }
    Audio {
        samples,
        sample_rate: SAMPLE_RATE,
    }
}

fn expected_times(delay_s: f64) -> Vec<f64> {
    let g = grid();
    (0..BARS)
        .flat_map(|bar| {
            RIFF.iter()
                .map(move |(slot, _)| bar as f64 * g.bar_s() + *slot as f64 * g.slot_s() + delay_s)
        })
        .collect()
}

#[test]
fn every_note_is_found_close_to_where_it_was_played() {
    let onsets = detect_onsets(&riff(0.0));
    for expected in expected_times(0.0) {
        let nearest = onsets
            .iter()
            .map(|o| (o.time_s - expected).abs())
            .fold(f64::INFINITY, f64::min);
        assert!(
            nearest < 0.010,
            "note à {expected:.3} s : attaque la plus proche à {:.1} ms",
            nearest * 1000.0
        );
    }
}

#[test]
fn nothing_clear_is_found_between_the_notes() {
    let onsets = detect_onsets(&riff(0.0));
    let expected = expected_times(0.0);
    for onset in onsets.iter().filter(|o| o.strength >= 0.25) {
        assert!(
            expected.iter().any(|e| (e - onset.time_s).abs() < 0.010),
            "attaque fantôme à {:.3} s (force {:.2})",
            onset.time_s,
            onset.strength
        );
    }
}

#[test]
fn the_folded_pattern_is_the_riff() {
    let audio = riff(0.0);
    let map = accent_map(&audio, &detect_onsets(&audio), grid());
    let played: Vec<usize> = RIFF.iter().map(|(slot, _)| *slot).collect();
    for (slot, &strength) in map.folded.iter().enumerate() {
        if played.contains(&slot) {
            assert!(strength >= 0.25, "case {slot} jouée mais lue à {strength:.2} ({})", map.pattern);
        } else {
            assert!(strength < 0.10, "case {slot} vide mais lue à {strength:.2} ({})", map.pattern);
        }
    }
    // Les deux notes les plus appuyées (cases 0 et 8) sortent en accents forts.
    let marks: Vec<char> = map.pattern.chars().filter(|c| *c != ' ').collect();
    assert!(marks[0] == 'X' && marks[8] == 'X', "{}", map.pattern);
}

/// La force lue est approchée : la résonance de la note précédente s'ajoute à la suivante.
/// On vérifie donc l'ordre, pas la valeur exacte.
#[test]
fn hard_notes_read_stronger_than_soft_ones() {
    let audio = riff(0.0);
    let map = accent_map(&audio, &detect_onsets(&audio), grid());
    let read = |gain_min: f64, gain_max: f64| -> Vec<f32> {
        RIFF.iter()
            .filter(|(_, gain)| (gain_min..=gain_max).contains(gain))
            .map(|(slot, _)| map.folded[*slot])
            .collect()
    };
    let weakest_hard = read(0.8, 1.0).into_iter().fold(f32::INFINITY, f32::min);
    let strongest_soft = read(0.0, 0.5).into_iter().fold(0.0f32, f32::max);
    assert!(
        weakest_hard > strongest_soft + 0.1,
        "note forte la plus faible : {weakest_hard:.2}, note légère la plus forte : {strongest_soft:.2} ({:?})",
        map.folded
    );
}

#[test]
fn playing_on_the_grid_reads_as_on_the_grid() {
    let audio = riff(0.0);
    let timing = accent_map(&audio, &detect_onsets(&audio), grid())
        .timing
        .expect("mise en place");
    assert!(
        timing.mean_offset_ms.abs() < 6.0 && timing.spread_ms < 6.0,
        "{timing:?}"
    );
}

#[test]
fn playing_late_reads_as_late() {
    let audio = riff(0.025);
    let timing = accent_map(&audio, &detect_onsets(&audio), grid())
        .timing
        .expect("mise en place");
    assert!(
        (timing.mean_offset_ms - 25.0).abs() < 6.0,
        "retard joué : 25 ms, retard lu : {:.1} ms",
        timing.mean_offset_ms
    );
}

/// Un passage qui commence au milieu d'une note qui résonne (ici 0,6 s après le début de la
/// mesure, pendant la note de la case 3).
fn cut(audio: &Audio, from_s: f64) -> Audio {
    Audio {
        samples: audio.samples[(from_s * SAMPLE_RATE as f64) as usize..].to_vec(),
        sample_rate: SAMPLE_RATE,
    }
}

fn grid_from(origin_s: f64) -> Grid {
    Grid {
        origin_s,
        ..grid()
    }
}

#[test]
fn a_passage_cut_mid_note_shows_a_false_attack_without_preroll() {
    // Ce test fixe le défaut que l'avance corrige : il échouerait si le défaut disparaissait.
    let passage = cut(&riff(0.0), 0.6);
    let map = accent_map(&passage, &detect_onsets(&passage), grid_from(-0.6));
    assert!(map.bars[0].slots[4] > 0.1, "{:?}", map.bars[0].slots);
}

#[test]
fn reading_a_little_before_the_passage_removes_the_false_attack() {
    let with_preroll = cut(&riff(0.0), 0.5);
    let (passage, onsets) = detect_onsets_after_preroll(&with_preroll, 0.1);
    let map = accent_map(&passage, &onsets, grid_from(-0.6));
    assert_eq!(map.bars[0].slots[4], 0.0, "{:?}", map.bars[0].slots);
    // Les vraies notes de la mesure sont toujours là (cases 6, 8, 10, 12).
    for slot in [6, 8, 10, 12] {
        assert!(map.bars[0].slots[slot] > 0.25, "case {slot} : {:?}", map.bars[0].slots);
    }
}

#[test]
fn a_note_on_the_first_beat_of_the_passage_is_kept() {
    // Passage qui commence pile sur le premier temps de la deuxième mesure.
    let bar_s = grid().bar_s();
    let with_preroll = cut(&riff(0.0), bar_s - 0.1);
    let (passage, onsets) = detect_onsets_after_preroll(&with_preroll, 0.1);
    let map = accent_map(&passage, &onsets, grid_from(0.0));
    assert!(map.bars[0].slots[0] > 0.6, "{:?}", map.bars[0].slots);
}
