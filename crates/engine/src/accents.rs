//! Carte d'accents : où tombent les attaques d'un extrait sur la grille du projet REAPER.
//!
//! On ne cherche pas le tempo dans l'audio : il vient du projet. On pose sa grille (mesures,
//! temps, doubles croches) sur l'extrait et on range chaque attaque dans la case la plus proche.

use serde::Serialize;

use crate::analysis::{grid_fit, GridFit};
use crate::onsets::Onset;
use crate::Audio;

/// La grille du projet, vue depuis l'extrait analysé.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Grid {
    /// Tempo en noires par minute.
    pub bpm: f64,
    /// Longueur d'une mesure en noires (4 pour du 4/4, 3 pour du 3/4, 3,5 pour du 7/8).
    pub beats_per_bar: f64,
    /// Cases par noire (4 = doubles croches, 3 = triolets de croches).
    pub slots_per_beat: u32,
    /// Position, en secondes dans l'extrait, d'un début de mesure. Peut être négative.
    pub origin_s: f64,
}

impl Grid {
    pub fn slot_s(&self) -> f64 {
        60.0 / self.bpm / self.slots_per_beat as f64
    }

    pub fn slots_per_bar(&self) -> usize {
        (self.beats_per_bar * self.slots_per_beat as f64).round().max(1.0) as usize
    }

    pub fn bar_s(&self) -> f64 {
        self.slot_s() * self.slots_per_bar() as f64
    }
}

/// Les accents d'une mesure.
#[derive(Debug, Clone, Serialize)]
pub struct BarAccents {
    /// Numéro de la mesure, 0 étant celle qui commence à `origin_s`.
    pub bar: i64,
    /// Force de l'attaque la plus forte dans chaque case, entre 0 et 1 (0 = rien).
    pub slots: Vec<f32>,
    /// Niveau sonore de la mesure, entre 0 et 1 (1 = la mesure la plus forte de l'extrait).
    pub energy: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccentMap {
    pub grid: Grid,
    pub bars: Vec<BarAccents>,
    /// Moyenne des mesures, case par case : le motif rythmique dominant de l'extrait.
    pub folded: Vec<f32>,
    /// `folded` en texte, un groupe par noire : `X` accent fort, `x` accent léger, `.` rien.
    pub pattern: String,
    /// Écart du jeu par rapport à la grille, mesuré sur les attaques nettes.
    pub timing: Option<GridFit>,
    pub onsets_used: usize,
}

/// Seuil au-dessus duquel une attaque compte pour mesurer la mise en place.
const CLEAR_ONSET: f32 = 0.25;
const STRONG_ACCENT: f32 = 0.6;
const LIGHT_ACCENT: f32 = 0.25;

pub fn accent_map(audio: &Audio, onsets: &[Onset], grid: Grid) -> AccentMap {
    let slots_per_bar = grid.slots_per_bar() as i64;
    let slot_s = grid.slot_s();
    let bar_s = grid.bar_s();
    let duration_s = audio.duration_s();

    let first_bar = ((0.0 - grid.origin_s) / bar_s).floor() as i64;
    let last_bar = (((duration_s - grid.origin_s) / bar_s).ceil() as i64 - 1).max(first_bar);
    let mut bars: Vec<BarAccents> = (first_bar..=last_bar)
        .map(|bar| BarAccents {
            bar,
            slots: vec![0.0; slots_per_bar as usize],
            energy: bar_rms(audio, grid.origin_s + bar as f64 * bar_s, bar_s),
        })
        .collect();

    let loudest = bars.iter().map(|b| b.energy).fold(0.0f32, f32::max);
    if loudest > 0.0 {
        for bar in &mut bars {
            bar.energy /= loudest;
        }
    }

    let mut onsets_used = 0;
    for onset in onsets {
        let global_slot = ((onset.time_s - grid.origin_s) / slot_s).round() as i64;
        let bar = global_slot.div_euclid(slots_per_bar);
        let slot = global_slot.rem_euclid(slots_per_bar) as usize;
        if let Some(accents) = bars.get_mut((bar - first_bar) as usize).filter(|_| bar >= first_bar)
        {
            accents.slots[slot] = accents.slots[slot].max(onset.strength);
            onsets_used += 1;
        }
    }

    let folded: Vec<f32> = (0..slots_per_bar as usize)
        .map(|slot| bars.iter().map(|b| b.slots[slot]).sum::<f32>() / bars.len() as f32)
        .collect();

    let clear_times: Vec<f64> = onsets
        .iter()
        .filter(|o| o.strength >= CLEAR_ONSET)
        .map(|o| o.time_s)
        .collect();
    let timing = grid_fit(
        &clear_times,
        grid.bpm * grid.slots_per_beat as f64,
        grid.origin_s,
    )
    // La mesure se fait à la case près, mais le tempo affiché reste celui du projet.
    .map(|fit| GridFit {
        project_bpm: grid.bpm,
        ..fit
    });

    AccentMap {
        pattern: pattern_text(&folded, grid.slots_per_beat as usize),
        grid,
        bars,
        folded,
        timing,
        onsets_used,
    }
}

fn pattern_text(folded: &[f32], slots_per_beat: usize) -> String {
    folded
        .chunks(slots_per_beat.max(1))
        .map(|beat| {
            beat.iter()
                .map(|&strength| {
                    if strength >= STRONG_ACCENT {
                        'X'
                    } else if strength >= LIGHT_ACCENT {
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

/// Niveau moyen (RMS) de l'audio entre `start_s` et `start_s + length_s`.
fn bar_rms(audio: &Audio, start_s: f64, length_s: f64) -> f32 {
    let rate = audio.sample_rate as f64;
    let from = ((start_s * rate).max(0.0) as usize).min(audio.samples.len());
    let to = (((start_s + length_s) * rate).max(0.0) as usize).min(audio.samples.len());
    if to <= from {
        return 0.0;
    }
    let sum: f64 = audio.samples[from..to]
        .iter()
        .map(|&s| s as f64 * s as f64)
        .sum();
    (sum / (to - from) as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_lengths_follow_tempo_and_meter() {
        let grid = Grid {
            bpm: 120.0,
            beats_per_bar: 3.5,
            slots_per_beat: 4,
            origin_s: 0.0,
        };
        assert_eq!(grid.slots_per_bar(), 14);
        assert!((grid.slot_s() - 0.125).abs() < 1e-12);
        assert!((grid.bar_s() - 1.75).abs() < 1e-12);
    }

    #[test]
    fn pattern_text_groups_by_beat() {
        let folded = [1.0, 0.0, 0.3, 0.0, 0.7, 0.1, 0.0, 0.0];
        assert_eq!(pattern_text(&folded, 4), "X.x. X...");
    }

    #[test]
    fn onsets_before_the_origin_land_in_a_negative_bar() {
        let audio = Audio {
            samples: vec![0.0; 44_100 * 4],
            sample_rate: 44_100,
        };
        let grid = Grid {
            bpm: 120.0,
            beats_per_bar: 4.0,
            slots_per_beat: 4,
            origin_s: 1.0,
        };
        // 0,5 s : une noire avant l'origine, donc dernière noire de la mesure -1 (case 12).
        let onsets = [Onset {
            time_s: 0.5,
            strength: 1.0,
        }];
        let map = accent_map(&audio, &onsets, grid);
        assert_eq!(map.bars[0].bar, -1);
        assert_eq!(map.bars[0].slots[12], 1.0);
        assert_eq!(map.onsets_used, 1);
    }
}
