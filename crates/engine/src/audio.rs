//! Décodage d'un fichier audio (WAV, FLAC, MP3, OGG, AAC…) en mono.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::EngineError;

/// Audio mono, échantillons entre -1 et 1.
pub struct Audio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Audio {
    pub fn duration_s(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }
}

/// Décode tout le fichier et mélange les canaux en mono.
pub fn decode_mono(path: &Path) -> Result<Audio, EngineError> {
    let file = File::open(path)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| EngineError::Decode(e.to_string()))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| EngineError::Decode("aucune piste audio dans le fichier".into()))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| EngineError::Decode(e.to_string()))?;

    let mut samples = Vec::new();
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut buffer: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // Fin de fichier.
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(EngineError::Decode(e.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // Paquet abîmé : on le saute.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(EngineError::Decode(e.to_string())),
        };
        let spec = *decoded.spec();
        sample_rate = spec.rate;
        let channels = spec.channels.count().max(1);
        let buf = buffer
            .get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        buf.copy_interleaved_ref(decoded);
        samples.extend(
            buf.samples()
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
    }

    if sample_rate == 0 || samples.is_empty() {
        return Err(EngineError::Decode("fichier audio vide".into()));
    }
    Ok(Audio {
        samples,
        sample_rate,
    })
}
