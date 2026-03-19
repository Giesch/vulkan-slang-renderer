#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "essentia",
# ]
# ///

# based on the official tutorial here:
# https://essentia.upf.edu/tutorial_rhythm_beatdetection.html

import essentia.standard as es

import json
import os
import sys
from pathlib import Path

def write_beats_json(audio_path_str):
    audio = es.MonoLoader(filename=audio_path_str)()
    rhythm_extractor = es.RhythmExtractor2013(method='multifeature')

    # https://essentia.upf.edu/reference/std_RhythmExtractor2013.html
    bpm, beats, beats_confidence, _estimates, beats_intervals = rhythm_extractor(audio)

    json_beats = {
        'bpm': bpm,
        'beats_confidence': beats_confidence,
        'beats': beats.tolist(),
        'beats_intervals': beats_intervals.tolist(),
    }

    json_path = audio_path_str.replace('flac', 'beats.json')
    with open(json_path, 'w', encoding='utf-8') as f:
        json.dump(json_beats, f, ensure_ascii=False, indent=4)


if __name__ == "__main__":
    audio_dir = sys.argv[1]
    audio_paths = Path(audio_dir).glob('**/*.flac')
    for audio_path in audio_paths:
        write_beats_json(str(audio_path))
