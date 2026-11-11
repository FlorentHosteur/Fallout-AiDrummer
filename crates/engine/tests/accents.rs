//! Vérifie la détection d'attaques et la carte d'accents sur un riff de synthèse dont on
//! connaît chaque note. Ne dit rien de la justesse sur un vrai instrument enregistré.

use aidrummer_engine::{accent_map, detect_onsets, Audio, Grid};

const SAMPLE_RATE: u32 = 44_100;
const BPM: f64 = 100.0;
const BARS: usize = 4;
/// Le riff d'une mesure : (case en doubles croches, force de la note).
const RIFF: [(usize, f64); 6] = [(0, 1.0), (3, 0.5), (6, 0.8), (8, 1.0), (10, 0.5), (12, 0.9)];

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
        for (slot, gain) in RIFF {
            let start_s = bar as f64 * bar_s + slot as f64 * slot_s + delay_s;
            let start = (start_s * SAMPLE_RATE as f64).round() as usize;
            for (i, sample) in samples.iter_mut().enumerate().skip(start) {
                let t = (i - start) as f64 / SAMPLE_RATE as f64;
                if t > 1.2 {
                    break;
                }
                let attack = (t / 0.003).min(1.0);
                let tone: f64 = [(110.0, 1.0), (220.0, 0.5), (330.0, 0.3), (880.0, 0.2)]
                    .iter()
                    .map(|(f, a)| a * (std::f64::consts::TAU * f * t).sin())
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
    // Les notes fortes (cases 0 et 8) ressortent plus que les notes légères (cases 3 et 10).
    assert!(map.folded[0] > map.folded[3] && map.folded[8] > map.folded[10], "{}", map.pattern);
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
