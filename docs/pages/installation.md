# Installation

## How to download

Download the latest release from the **Assets** section of the [release page](https://github.com/Quentin-Piot/piano-pro/releases).

Available for Windows, macOS, and Ubuntu.

## How to run

### On Mac

- Extract
- Right click + open

### On Windows/Linux

- Extract and double click

#### How to get working audio playback

(on macOS audio out is included out of the box)

- Download [default.sf2](https://github.com/Quentin-Piot/piano-pro/blob/master/default.sf2)
- On Windows:
    - Place `default.sf2` in the same dir as executable (to make it a default one)
- On Linux:
    - Place `default.sf2` in the PianoPro configuration directory `~/.config/pianopro` (to make it a default one)
- Or select any sf2 file from the in-app menu

## How to convert audio to MIDI

PianoPro can transcribe audio files (WAV, MP3) into MIDI automatically using AI.

1. Launch PianoPro
2. From the Main Menu, click **Import Audio** (or press **`a`**)
3. Select an audio file
4. Click **Convert** to transcribe the audio
5. The generated MIDI will be saved to your library and loaded for playback

This uses the bundled Basic Pitch model, which works best with clear, solo piano recordings.
