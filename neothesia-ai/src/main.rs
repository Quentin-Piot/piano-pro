mod args;

fn main() -> anyhow::Result<()> {
    let args = args::Args::get_from_env()?;

    let midi = if let Some(model) = &args.model {
        neothesia_ai::transcribe_audio_to_midi_with_model(&args.input, model)?
    } else {
        neothesia_ai::transcribe_audio_to_midi(&args.input)?
    };

    midi.save(&args.output)?;

    Ok(())
}
