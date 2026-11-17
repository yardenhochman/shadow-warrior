#!/usr/bin/env python3
"""Turn your own recordings into demon-shadow clips (no TTS needed).

Record each line as a mono or stereo WAV (phone voice memo exported to WAV is fine), name it
after the line id (e.g. weak1.wav, presence1.wav), drop them in one folder and run:

    uv run --with numpy python demonize.py IN_DIR [OUT_DIR]   # OUT_DIR defaults to ../voice/mine

Each file goes through the same chain as the Hebrew TTS taunts (calm voice on top, sub-octave
growl creeping in, temple reverb), then to -16 LUFS / true peak < -1 dBTP mp3. Tips: record
calm and close to the mic, a little slower than you would normally talk; leave 0.3 s of silence
before and after. --style monk gives the slower full-ramp monk version instead.
"""
import argparse, os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import render_voices as rv

ap = argparse.ArgumentParser()
ap.add_argument('inp'); ap.add_argument('out', nargs='?', default=os.path.join(rv.OUT, 'mine'))
ap.add_argument('--style', choices=['hebrew', 'monk'], default='hebrew')
a = ap.parse_args()
os.makedirs(a.out, exist_ok=True)
with tempfile.TemporaryDirectory() as tmp:
    rv.make_ir(f'{tmp}/ir_temple.wav', 3.5, 1.6, 2500, predelay=0.03, seed=3)
    for f in sorted(os.listdir(a.inp)):
        if not f.lower().endswith(('.wav', '.flac', '.mp3', '.m4a', '.ogg', '.opus')):
            continue
        src = f'{tmp}/in.wav'
        rv.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', os.path.join(a.inp, f), '-ac', '1',
                '-af', 'silenceremove=start_periods=1:start_threshold=-45dB,areverse,silenceremove=start_periods=1:start_threshold=-45dB,areverse',
                src])
        kw = {} if a.style == 'monk' else rv.HE_DEMON
        mix = rv.demon_chain(src, tmp, **kw)
        dst = os.path.join(a.out, os.path.splitext(f)[0] + '.mp3')
        lufs, tp = rv.finish(mix, dst)
        print(f'{dst}: {lufs:.1f} LUFS, {tp:.1f} dBTP')
