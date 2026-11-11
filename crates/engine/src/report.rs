//! Compte rendu lisible de l'analyse d'une sélection du projet.

use serde::Serialize;

use crate::accents::{AccentMap, Grid};
use crate::drums::DrumReading;

/// Les accents d'une piste sur la sélection.
#[derive(Debug, Clone, Serialize)]
pub struct TrackAccents {
    pub name: String,
    pub accents: AccentMap,
}

/// Tout ce que l'analyse d'une sélection a trouvé.
#[derive(Debug, Clone, Serialize)]
pub struct SelectionAnalysis {
    /// Début et fin de la sélection, en secondes dans le projet.
    pub start_s: f64,
    pub end_s: f64,
    pub grid: Grid,
    /// Vrai si le tempo change dans la sélection : la grille est alors une moyenne.
    pub tempo_changes: bool,
    pub tracks: Vec<TrackAccents>,
    /// Pistes sans son dans la sélection.
    pub silent_tracks: Vec<String>,
    /// Parties de batterie MIDI trouvées dans la sélection.
    pub drums: Vec<DrumReading>,
}

/// Le compte rendu en texte, pour la console de REAPER.
pub fn selection_report(analysis: &SelectionAnalysis) -> String {
    let grid = &analysis.grid;
    let bars = (analysis.end_s - analysis.start_s) / grid.bar_s();
    let mut text = format!(
        "AI Drummer : analyse de la sélection\n  de {:.3} s à {:.3} s · {:.2} BPM · mesure de {} noires · {:.1} mesures\n",
        analysis.start_s,
        analysis.end_s,
        grid.bpm,
        trim_number(grid.beats_per_bar),
        bars,
    );
    if analysis.tempo_changes {
        text.push_str(
            "  Attention : le tempo change dans la sélection. La grille utilisée est une moyenne, les accents peuvent être décalés.\n",
        );
    }
    if analysis.tracks.is_empty() {
        text.push_str("  Aucune piste ne contient de son dans la sélection.\n");
    } else {
        text.push_str("  Motif dominant par piste (X accent fort, x accent léger, . rien) :\n");
    }

    let name_width = analysis
        .tracks
        .iter()
        .map(|t| t.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(28);
    for track in &analysis.tracks {
        let name: String = track.name.chars().take(name_width).collect();
        let timing = match &track.accents.timing {
            Some(fit) => format!(
                "{} de {:.0} ms, régularité {:.0} ms",
                if fit.mean_offset_ms >= 0.0 { "retard" } else { "avance" },
                fit.mean_offset_ms.abs(),
                fit.spread_ms
            ),
            None => "pas d'attaque nette".to_string(),
        };
        text.push_str(&format!(
            "    {name:<name_width$}  {}   {timing}\n",
            track.accents.pattern
        ));
    }
    if !analysis.silent_tracks.is_empty() {
        text.push_str(&format!(
            "  Sans son dans la sélection : {}\n",
            analysis.silent_tracks.join(", ")
        ));
    }
    for drums in &analysis.drums {
        text.push_str(&drum_report(drums));
    }
    text
}

fn drum_report(drums: &DrumReading) -> String {
    let hits: usize = drums.lanes.iter().map(|lane| lane.hits).sum();
    let mut text = format!(
        "  Batterie MIDI « {} » ({hits} coups ; X dans la plupart des mesures, x dans certaines) :\n",
        drums.track
    );
    let label_width = drums
        .lanes
        .iter()
        .map(|lane| lane.piece.label().chars().count())
        .max()
        .unwrap_or(0);
    for lane in &drums.lanes {
        text.push_str(&format!(
            "    {:<label_width$}  {}\n",
            lane.piece.label(),
            lane.pattern
        ));
    }
    if !drums.unknown_notes.is_empty() {
        let list: Vec<String> = drums
            .unknown_notes
            .iter()
            .map(|(note, count)| format!("{note} ({count} coup{})", if *count > 1 { "s" } else { "" }))
            .collect();
        text.push_str(&format!(
            "    Notes absentes de la table, rangées dans « autres notes » : {}\n",
            list.join(", ")
        ));
    }
    if let Some(share) = drums.accents_covered {
        text.push_str(&format!(
            "    Accents forts du morceau repris par la grosse caisse ou la caisse claire : {:.0} %\n",
            share * 100.0
        ));
    }
    if let Some(share) = drums.kicks_on_accents {
        text.push_str(&format!(
            "    Coups de grosse caisse posés sur un accent du morceau : {:.0} %\n",
            share * 100.0
        ));
    }
    text
}

/// 4.0 s'écrit « 4 », 3.5 s'écrit « 3.5 ».
fn trim_number(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
            .trim_end_matches('0')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accents::accent_map;
    use crate::onsets::Onset;
    use crate::Audio;

    fn analysis() -> SelectionAnalysis {
        let grid = Grid {
            bpm: 120.0,
            beats_per_bar: 4.0,
            slots_per_beat: 4,
            origin_s: 0.0,
        };
        let audio = Audio {
            samples: vec![0.1; 44_100 * 4],
            sample_rate: 44_100,
        };
        // Une attaque forte sur chaque temps, jouée 10 ms en retard.
        let onsets: Vec<Onset> = (0..8)
            .map(|beat| Onset {
                time_s: beat as f64 * 0.5 + 0.010,
                strength: 1.0,
            })
            .collect();
        SelectionAnalysis {
            start_s: 8.0,
            end_s: 12.0,
            grid,
            tempo_changes: false,
            tracks: vec![TrackAccents {
                name: "Guitare".to_string(),
                accents: accent_map(&audio, &onsets, grid),
            }],
            silent_tracks: vec!["Clic".to_string()],
            drums: Vec::new(),
        }
    }

    #[test]
    fn report_shows_the_grid_the_pattern_and_the_timing() {
        let text = selection_report(&analysis());
        assert!(text.contains("120.00 BPM · mesure de 4 noires · 2.0 mesures"), "{text}");
        assert!(text.contains("Guitare  X... X... X... X...   retard de 10 ms, régularité 0 ms"), "{text}");
        assert!(text.contains("Sans son dans la sélection : Clic"), "{text}");
        assert!(!text.contains("Attention"), "{text}");
    }

    #[test]
    fn report_warns_when_the_tempo_changes() {
        let mut analysis = analysis();
        analysis.tempo_changes = true;
        assert!(selection_report(&analysis).contains("le tempo change dans la sélection"));
    }

    #[test]
    fn report_says_so_when_nothing_sounds() {
        let mut analysis = analysis();
        analysis.tracks.clear();
        assert!(selection_report(&analysis).contains("Aucune piste ne contient de son"));
    }

    #[test]
    fn report_shows_an_existing_drum_part() {
        use crate::drums::{read_drums, DrumNote};
        let mut analysis = analysis();
        // Grosse caisse sur chaque temps des deux mesures, plus une note inconnue.
        let mut part: Vec<DrumNote> = (0..8)
            .map(|beat| DrumNote {
                position_qn: beat as f64,
                note: 36,
                velocity: 100,
            })
            .collect();
        part.push(DrumNote {
            position_qn: 0.5,
            note: 31,
            velocity: 90,
        });
        let maps: Vec<_> = analysis.tracks.iter().map(|t| t.accents.clone()).collect();
        analysis.drums = vec![read_drums("drums", &part, &analysis.grid, &maps)];
        let text = selection_report(&analysis);
        assert!(text.contains("Batterie MIDI « drums » (9 coups"), "{text}");
        assert!(text.contains("grosse caisse  X... X... X... X..."), "{text}");
        assert!(text.contains("rangées dans « autres notes » : 31 (1 coup)"), "{text}");
        assert!(text.contains("repris par la grosse caisse ou la caisse claire : 100 %"), "{text}");
        assert!(text.contains("posés sur un accent du morceau : 100 %"), "{text}");
    }

    #[test]
    fn odd_meters_keep_their_fraction() {
        assert_eq!(trim_number(4.0), "4");
        assert_eq!(trim_number(3.5), "3.5");
    }
}
