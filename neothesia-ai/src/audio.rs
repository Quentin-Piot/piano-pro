use std::path::Path;

use symphonium::{ResampleQuality, SymphoniumLoader};

pub fn load(
    path: impl AsRef<Path>,
    sample_rate: u32,
    pad_to_samples: Option<usize>,
) -> anyhow::Result<Vec<f32>> {
    // A struct used to load audio files.
    let mut loader = SymphoniumLoader::new();

    let audio_data_f32 = loader
        .load_f32(path, Some(sample_rate), ResampleQuality::High, None)
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    let mut channels = audio_data_f32.data;
    let mut mono = match channels.len() {
        0 => anyhow::bail!("audio file has no channels"),
        1 => channels.remove(0),
        channel_count => {
            let samples_len = channels[0].len();
            let mut mono = vec![0.0; samples_len];

            for channel in channels {
                for (sample, mixed) in channel.into_iter().zip(&mut mono) {
                    *mixed += sample / channel_count as f32;
                }
            }

            mono
        }
    };

    if let Some(pad_to_samples) = pad_to_samples {
        let pad_len = (mono.len() as f32 / pad_to_samples as f32).ceil() as usize * pad_to_samples
            - mono.len();

        mono.resize(mono.len() + pad_len, 0.0);
    }

    Ok(mono)
}
