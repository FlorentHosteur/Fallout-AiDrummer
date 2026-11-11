//! Détection des attaques (débuts de notes) dans un extrait audio.
//!
//! Méthode : flux spectral. On découpe l'audio en fenêtres courtes, on regarde le spectre de
//! chacune, et on mesure de combien l'énergie augmente d'une fenêtre à la suivante. Une attaque
//! (coup de médiator, note de basse, accord plaqué) produit un pic de cette augmentation.

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use serde::Serialize;

use crate::Audio;

/// Une attaque détectée.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Onset {
    /// Position en secondes depuis le début de l'extrait.
    pub time_s: f64,
    /// Force relative, entre 0 et 1 : le gain de niveau sonore apporté par l'attaque,
    /// rapporté à celui des attaques fortes de l'extrait.
    pub strength: f32,
}

/// Durée visée d'une fenêtre d'analyse.
const WINDOW_S: f64 = 0.023;
/// Une attaque doit dépasser la moyenne locale du flux de cette marge (flux normalisé à 1).
const THRESHOLD_ABOVE_LOCAL_MEAN: f32 = 0.04;
/// Largeur de la moyenne locale, de part et d'autre.
const LOCAL_MEAN_S: f64 = 0.10;
/// Deux attaques ne peuvent pas être plus proches que cela ; la plus forte l'emporte.
const MIN_GAP_S: f64 = 0.045;

pub fn detect_onsets(audio: &Audio) -> Vec<Onset> {
    let sample_rate = audio.sample_rate as f64;
    let window = ((WINDOW_S * sample_rate) as usize)
        .next_power_of_two()
        .max(256);
    let hop = window / 4;
    let flux = spectral_flux(&audio.samples, window, hop);
    let Some(max_flux) = flux.iter().copied().reduce(f32::max).filter(|m| *m > 0.0) else {
        return Vec::new();
    };

    let hop_s = hop as f64 / sample_rate;
    let mean_radius = ((LOCAL_MEAN_S / hop_s).round() as usize).max(1);
    let peak_radius = 3usize;
    let mut onsets: Vec<Onset> = Vec::new();

    for n in 0..flux.len() {
        let value = flux[n] / max_flux;
        let lo = n.saturating_sub(peak_radius);
        let hi = (n + peak_radius + 1).min(flux.len());
        if flux[lo..hi].iter().any(|&other| other > flux[n]) {
            continue;
        }
        let mean_lo = n.saturating_sub(mean_radius);
        let mean_hi = (n + mean_radius + 1).min(flux.len());
        let local_mean =
            flux[mean_lo..mean_hi].iter().sum::<f32>() / (mean_hi - mean_lo) as f32 / max_flux;
        if value < local_mean + THRESHOLD_ABOVE_LOCAL_MEAN {
            continue;
        }
        // L'audio est précédé d'une fenêtre de silence (voir `spectral_flux`) : la fenêtre n
        // commence donc à l'échantillon n * hop - window de l'extrait. Le flux culmine quand
        // l'attaque vient d'entrer par la fin de la fenêtre, d'où la position retenue.
        let time_s = ((n * hop) as f64 - hop as f64 * ATTACK_LAG_HOPS) / sample_rate;
        let onset = Onset {
            time_s: time_s.max(0.0),
            strength: value,
        };
        match onsets.last_mut() {
            Some(last) if onset.time_s - last.time_s < MIN_GAP_S => {
                if onset.strength > last.strength {
                    *last = onset;
                }
            }
            _ => onsets.push(onset),
        }
    }

    // Le flux sert à dater les attaques, pas à dire leur force : il est compressé, et une note
    // qui sort du silence y pèse plus qu'une note aussi forte jouée sur la résonance de la
    // précédente. La force retenue est le gain de niveau sonore que l'attaque apporte.
    for onset in &mut onsets {
        onset.strength = level_rise(audio, onset.time_s);
    }
    // Référence : le niveau des attaques fortes de l'extrait (90e centile), pas la seule plus
    // forte, pour qu'un coup isolé plus appuyé n'écrase pas tous les autres.
    let mut sorted: Vec<f32> = onsets.iter().map(|o| o.strength).collect();
    sorted.sort_by(f32::total_cmp);
    let reference = sorted
        .get(((sorted.len().saturating_sub(1)) as f64 * 0.9).floor() as usize)
        .copied()
        .unwrap_or(0.0);
    if reference <= 0.0 {
        return Vec::new();
    }
    for onset in &mut onsets {
        onset.strength = (onset.strength / reference).min(1.0);
    }
    onsets.retain(|onset| onset.strength >= MIN_STRENGTH);
    onsets
}

/// Niveau mesuré juste avant l'attaque, pour le comparer à celui qui suit.
const BEFORE_S: f64 = 0.030;
/// Niveau mesuré juste après l'attaque.
const AFTER_S: f64 = 0.050;
/// Marge autour de l'instant détecté, pour ne pas compter l'attaque dans le « avant ».
const GUARD_S: f64 = 0.008;
/// En dessous de cette force relative, ce n'est pas une note : on l'écarte.
const MIN_STRENGTH: f32 = 0.03;

/// De combien le niveau sonore monte à l'instant `time_s` (0 s'il ne monte pas).
fn level_rise(audio: &Audio, time_s: f64) -> f32 {
    let before = mean_square(audio, time_s - GUARD_S - BEFORE_S, time_s - GUARD_S);
    let after = mean_square(audio, time_s - GUARD_S, time_s - GUARD_S + AFTER_S);
    (after - before).max(0.0).sqrt() as f32
}

fn mean_square(audio: &Audio, from_s: f64, to_s: f64) -> f64 {
    let rate = audio.sample_rate as f64;
    let from = ((from_s * rate).max(0.0) as usize).min(audio.samples.len());
    let to = ((to_s * rate).max(0.0) as usize).min(audio.samples.len());
    if to <= from {
        return 0.0;
    }
    audio.samples[from..to]
        .iter()
        .map(|&s| s as f64 * s as f64)
        .sum::<f64>()
        / (to - from) as f64
}

/// Détecte les attaques d'un extrait lu avec un peu d'avance (`preroll_s` secondes avant le
/// passage voulu), puis retire cette avance : renvoie le passage seul et ses attaques, datées
/// depuis son début.
///
/// Sans cette avance, un passage qui commence au milieu d'une note qui résonne produit une
/// fausse attaque à son tout début : le son semble surgir du silence.
pub fn detect_onsets_after_preroll(audio: &Audio, preroll_s: f64) -> (Audio, Vec<Onset>) {
    let skip =
        ((preroll_s * audio.sample_rate as f64).round().max(0.0) as usize).min(audio.samples.len());
    let skipped_s = skip as f64 / audio.sample_rate as f64;
    let onsets = detect_onsets(audio)
        .into_iter()
        // Une note jouée un rien en avance sur le début du passage lui appartient encore.
        .filter(|onset| onset.time_s >= skipped_s - EARLY_NOTE_S)
        .map(|onset| Onset {
            time_s: onset.time_s - skipped_s,
            ..onset
        })
        .collect();
    let passage = Audio {
        samples: audio.samples[skip..].to_vec(),
        sample_rate: audio.sample_rate,
    };
    (passage, onsets)
}

/// Avance tolérée pour une note jouée juste avant le début du passage.
const EARLY_NOTE_S: f64 = 0.030;

/// Retard, en nombre de pas, entre l'entrée d'une attaque dans la fenêtre et le pic de flux.
/// Mesuré sur des clics de synthèse (voir les tests).
const ATTACK_LAG_HOPS: f64 = 0.5;

/// Augmentation d'énergie spectrale d'une fenêtre à la suivante.
fn spectral_flux(samples: &[f32], window: usize, hop: usize) -> Vec<f32> {
    // Une fenêtre de silence devant, pour qu'une attaque au tout début soit vue comme telle.
    let mut padded = vec![0.0f32; window];
    padded.extend_from_slice(samples);
    padded.extend(std::iter::repeat_n(0.0, window));

    let hann: Vec<f32> = (0..window)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / window as f32).cos())
        .collect();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(window);
    let bins = window / 2;
    let mut buffer = vec![Complex::new(0.0f32, 0.0); window];
    let mut previous = vec![0.0f32; bins];
    let mut flux = Vec::with_capacity(padded.len() / hop);

    let mut start = 0;
    while start + window <= padded.len() {
        for (slot, (&sample, &weight)) in buffer
            .iter_mut()
            .zip(padded[start..start + window].iter().zip(&hann))
        {
            *slot = Complex::new(sample * weight, 0.0);
        }
        fft.process(&mut buffer);
        let mut rise = 0.0f32;
        for (bin, previous_magnitude) in previous.iter_mut().enumerate() {
            // Compression logarithmique : une attaque douce sur un passage calme compte aussi.
            let magnitude = (1.0 + 100.0 * buffer[bin].norm()).ln();
            rise += (magnitude - *previous_magnitude).max(0.0);
            *previous_magnitude = magnitude;
        }
        flux.push(rise);
        start += hop;
    }
    flux
}
