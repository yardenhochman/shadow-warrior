#!/usr/bin/env python3
"""Render the Sound Lab announcer clips: Piper TTS (offline, open source) -> ffmpeg chains.

Every character is original: a stock Piper voice is only raw material, and the chains
below (pitch/formant shifts, sub-octave growl, choir layering, crushing, reverbs) make
something no single speaker sounds like.

Setup (models are NOT committed; they live outside the repo):
    uv venv .venv && uv pip install --python .venv piper-tts soundfile phonikud phonikud-onnx piper-onnx
    .venv/bin/python -m piper.download_voices --data-dir VOICES en_US-ryan-high en_GB-alan-medium \
        en_US-joe-medium en_GB-jenny_dioco-medium en_GB-northern_english_male-medium
    Hebrew (optional), into HE: phonikud-1.0.int8.onnx from huggingface.co/Phonikud/phonikud-onnx,
        michael.onnx + model.config.json from huggingface.co/Phonikud/phonikud-tts-checkpoints
Run:
    .venv/bin/python render_voices.py --voices VOICES [--he-voices HE] [--only monk|hebrew|laugh|...]
Own recordings through the demon chain: see demonize.py.
Output: ../voice/<character>/<line>.mp3 (mono, ~-16 LUFS, true peak < -1 dBTP) and
../voice/manifest.json (text, duration, measured loudness and peak per clip).
"""
import argparse, json, os, re, subprocess, sys, tempfile, wave
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.normpath(os.path.join(HERE, '..', 'voice'))
SR = 44100
TARGET_LUFS = -16.0
TP_CEIL = -1.5

# (id, category, text). Categories match the page's line slots.
LINES = {
    'monk': [  # Demon monk: a meditation teacher's calm, slowly rotting into a growl
        ('round1', 'Round start', 'Breathe in. Hold it. Round one.'),
        ('fight', 'Fight', 'Now. Breathe out. And fight.'),
        ('energy', 'Energy full', 'Your breath is full. The fire is awake.'),
        ('first_heavy', 'First heavy', 'There. The first wound. Feel how good it is.'),
        ('combo3', 'Combo 3', 'Three. Let it flow.'),
        ('combo5', 'Combo 5', 'Five. Do not think. Only strike.'),
        ('combo10', 'Combo 10', 'Ten. You are not the storm. You are the end of it.'),
        ('low', 'Shadow low HP', 'It is cracking. Do not let go of it.'),
        ('ko', 'K.O. / victory', 'Be still now. Knocked out.'),
        ('round2', 'Round 2+', 'Again. Round two. Deeper this time.'),
        ('time', 'Time running out', 'Ten breaths remain. Spend them.'),
        ('defeat', 'Game over', 'Let it go. Let it all go. Game over.'),
        ('taunt1', 'Taunt', 'Your fists are soft. Your shadow is not.'),
        ('taunt2', 'Taunt', 'Inhale the doubt. Exhale the beast.'),
    ],
    'announcer': [  # Arena barker: big hype, slapback, stadium tail
        ('round1', 'Round start', 'From the dark side of the room. Round one!'),
        ('fight', 'Fight', 'Now... fight!'),
        ('energy', 'Energy full', 'Fully charged!'),
        ('first_heavy', 'First heavy', 'First blood!'),
        ('combo3', 'Combo 3', 'Triple strike!'),
        ('combo5', 'Combo 5', 'Five hit flurry!'),
        ('combo10', 'Combo 10', 'Ten hits! Absolute frenzy!'),
        ('low', 'Shadow low HP', 'The shadow is wobbling!'),
        ('ko', 'K.O. / victory', 'K.O.! The shadow is down!'),
        ('round2', 'Round 2+', 'Round two! Hands up!'),
        ('time', 'Time running out', 'Ten seconds! Empty the tank!'),
        ('defeat', 'Game over', 'Time! The shadow survives. Game over.'),
        ('taunt1', 'Taunt', 'Is that a punch or a handshake?'),
    ],
    'choir': [  # The shadow itself: a whispering choir of you, with a reversed reverb swell
        ('round1', 'Round start', 'I am what you left behind. Round one.'),
        ('fight', 'Fight', 'Come. Hit what you are.'),
        ('energy', 'Energy full', 'So loud. I heard every word.'),
        ('first_heavy', 'First heavy', 'That one. I felt that one.'),
        ('combo3', 'Combo 3', 'Again?'),
        ('combo5', 'Combo 5', 'Stop. Stop it.'),
        ('combo10', 'Combo 10', 'What... are you?'),
        ('low', 'Shadow low HP', 'I am fading. No. Not yet.'),
        ('ko', 'K.O. / victory', 'You will not always win.'),
        ('round2', 'Round 2+', 'I came back. Round two.'),
        ('time', 'Time running out', 'Tick. Tock. Tick.'),
        ('defeat', 'Game over', 'Mine again. Game over.'),
        ('taunt1', 'Taunt', 'Is that all?'),
        ('taunt2', 'Taunt', 'I can wait all night.'),
        ('taunt3', 'Taunt', 'You hit like a whisper.'),
    ],
    'hebrew': [  # the shadow as a butler-demon: perfect manners, filthy underneath; gender-balanced
        # (id, trigger, text, English gloss). Placeholders: the real ones get recorded (demonize.py / the lab).
        ('zero1', 'Pre-fight: presence / WARMING starts', 'סליחה... אַתְּ... אַתָּה... לא משנה. אפס.', 'Excuse me... you (f)... you (m)... never mind. Zero.'),
        ('zero2', 'Pre-fight: presence / WARMING starts', 'סליחה, את... או אתה? לא חשוב. אפס, בשני המינים.', 'Excuse me, you (f)... or you (m)? Does not matter. A zero, in both genders.'),
        ('zero3', 'Pre-fight: presence / WARMING starts', 'סליחה... אַתְּ, אַתָּה, אַתֶּם... אפס.', 'Excuse me... you, you, all of you... zero.'),
        ('welcome1', 'Pre-fight: presence / WARMING starts', 'ברוכים הבאים. נא לנגב את הרגליים, ואת הכבוד העצמי.', 'Welcome. Kindly wipe your feet. And your self-respect.'),
        ('welcome2', 'Pre-fight: presence / WARMING starts', 'אורחים! איזה כבוד. איזה ריח.', 'Guests! What an honour. What a smell.'),
        ('idle1', 'Idle / hesitating mid-fight', 'מה, נגמר הסוס?', 'What, out of horse already? (out of steam)'),
        ('idle2', 'Idle / hesitating mid-fight', 'מה קרה, נתקענו? אפשר להביא כיסא.', 'What happened, stuck? I can fetch you a chair.'),
        ('idle3', 'Idle / hesitating mid-fight', 'בבקשה, קחו את הזמן. אני מחכה בסבלנות. ובגועל.', 'Please, take your time. I wait patiently. And with disgust.'),
        ('idle4', 'Idle / hesitating mid-fight', 'תזכורת קטנה ומנומסת: את האגרופים זורקים, לא חושבים עליהם.', 'A small polite reminder: punches are thrown, not thought about.'),
        ('stamina1', 'Stamina: punch rate drops', 'מתנשפים כבר? חמוד. ממש חמוד.', 'Panting already? Cute. Really cute.'),
        ('stamina2', 'Stamina: punch rate drops', 'נגמר לְךָ... סליחה, לָךְ... נו, למישהו פה נגמר הסוס.', 'You (m) ran out... sorry, you (f)... well, someone here ran out of horse.'),
        ('stamina3', 'Stamina: punch rate drops', 'אין לחץ. אני רק אזדקן פה בנחת.', 'No pressure. I will just grow old here, comfortably.'),
        ('weak1', 'Weak punch', 'מה זה, התחפשת למלכה אסתר?', 'What is this, dressed up (m)... (f)... as Queen Esther?'),
        ('weak2', 'Weak punch', 'סליחה על השאלה: זה היה אגרוף, או ליטוף?', 'Pardon the question: was that a punch, or a caress?'),
        ('weak3', 'Weak punch', 'תודה רבה על העיסוי. ממש נעים.', 'Thank you so much for the massage. Very pleasant.'),
        ('weak4', 'Weak punch', 'מרגש. באמת. עוד מעט אולי ארגיש משהו.', 'Moving. Truly. Any minute now I might feel something.'),
        ('lose1', 'Player loses / time out', 'נגמר הזמן. היה לי לעונג. לכם פחות.', 'Time is up. It was my pleasure. Less so yours.'),
        ('lose2', 'Player loses / time out', 'תודה שבאתם. היציאה משמאל, הבושה מימין.', 'Thank you for coming. The exit is on the left, the shame on the right.'),
        ('rise1', 'Shadow rises, new round', 'חשבתם שזה נגמר? איזה חמודים.', 'You thought it was over? How adorable.'),
        ('rise2', 'Shadow rises, new round', 'קמתי. סליחה על ההפרעה. עכשיו אני ממש עצבני.', 'I am up. Sorry for the interruption. Now I am truly annoyed.'),
        ('win1', 'Player wins (bonus)', 'כל הכבוד. באמת. אני אזכור את הפרצופים.', 'Well done. Truly. I will remember your faces.'),
    ],
    'arcade': [  # Bonus: a 1987 arcade cabinet that has been left on for forty years
        ('round1', 'Round start', 'Round one!'),
        ('fight', 'Fight', 'Fight!'),
        ('ko', 'K.O. / victory', 'Knockout! Perfect.'),
        ('defeat', 'Game over', 'Game over. Insert coin.'),
    ],
}


def run(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode:
        sys.exit(f'failed: {" ".join(cmd)}\n{r.stderr[-2000:]}')
    return r


def write_wav(path, x, sr=SR):
    x = np.clip(x, -1, 1)
    with wave.open(path, 'wb') as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
        w.writeframes((x * 32767).astype('<i2').tobytes())


def make_ir(path, seconds, decay, lowpass_hz, predelay=0.0, seed=1):
    """Exponentially decaying noise, one-pole low-passed, as a reverb impulse response."""
    rng = np.random.default_rng(seed)
    n = int(seconds * SR); t = np.arange(n) / SR
    x = rng.standard_normal(n) * np.exp(-decay * t)
    a = np.exp(-2 * np.pi * lowpass_hz / SR); y = np.empty_like(x); s = 0.0
    for i in range(n):
        s = (1 - a) * x[i] + a * s; y[i] = s
    y = np.concatenate([np.zeros(int(predelay * SR)), y])
    y /= np.sqrt(np.sum(y ** 2)) + 1e-9
    write_wav(path, y * 0.5)


# How a line is spelled for the TTS when the display text trips it up (short words under heavy processing).
# 'K.O.' is the hardest word for the TTS; this spelling was the one Whisper heard as K.O. (pick takes by ear or ASR).
SPOKEN = {'K.O.! The shadow is down!': 'Kay... Oh! The shadow is down!', 'Round one!': 'Round 1!'}


def tts(voices, model, text, out, length=1.0, noise=0.5, noise_w=0.7):
    text = SPOKEN.get(text, text)
    p = subprocess.run([sys.executable, '-m', 'piper', '-m', os.path.join(voices, model + '.onnx'), '-f', out,
                        '--length-scale', str(length), '--noise-scale', str(noise), '--noise-w-scale', str(noise_w),
                        '--sentence-silence', '0.25'], input=text, capture_output=True, text=True)
    if p.returncode:
        sys.exit(p.stderr)


def ff(inputs, graph, out):
    cmd = ['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y']
    for i in inputs:
        cmd += ['-i', i]
    run(cmd + ['-filter_complex', graph, '-map', '[out]', '-ac', '1', '-ar', str(SR), '-c:a', 'pcm_f32le', out])


def duration(path):
    r = run(['ffprobe', '-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', path])
    return float(r.stdout.strip())


def measure(path):
    """Integrated loudness (LUFS) and true peak (dBTP) via ffmpeg ebur128."""
    r = subprocess.run(['ffmpeg', '-hide_banner', '-nostats', '-i', path, '-af', 'ebur128=peak=true:framelog=quiet',
                        '-f', 'null', '-'], capture_output=True, text=True)
    s = r.stderr
    i = re.findall(r'I:\s+(-?[\d.]+|-inf) LUFS', s)
    tp = re.findall(r'Peak:\s+(-?[\d.]+|-inf) dBFS', s)
    return float(i[-1]) if i else None, float(tp[-1]) if tp else None


def finish(src, dst):
    """Gain to -16 LUFS, brickwall at the TP ceiling, mp3 64k mono; re-check the encoded file."""
    lufs, _ = measure(src)
    gain = TARGET_LUFS - lufs
    ceil = TP_CEIL
    for _ in range(4):
        lim = 10 ** ((ceil - 0.5) / 20)
        run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', src, '-af',
             f'volume={gain:.2f}dB,aresample=176400,alimiter=limit={lim:.4f}:attack=1:release=60:level=disabled,aresample={SR},'
             'apad=pad_dur=0.05', '-ac', '1', '-c:a', 'libmp3lame', '-b:a', '64k', dst])
        lufs2, tp = measure(dst)
        if tp is not None and tp < -1.0:
            return lufs2, tp
        ceil -= 0.7
    return lufs2, tp


def demon_chain(raw, tmp, out=None, growl_from=0.15, growl_to=1.1, ramp=0.75, calm_pitch=0.9, growl_pitch=0.5, pad=2.2):
    """The demon-monk voice: calm teacher on top, a sub-octave growl creeping in underneath.

    calm: slightly lower, warm, close; breath: the whisper band of the same voice;
    growl: an octave down with dropped formants, tanh-driven, 38 Hz gravel tremolo,
    fading from growl_from to growl_to over `ramp` of the line, so it starts serene and
    ends possessed. Works on any mono WAV (TTS output or a home recording)."""
    out = out or f'{tmp}/mix.wav'
    d = duration(raw) + pad
    g = (
        f'[0:a]aresample={SR},apad=pad_dur={pad},asplit=3[a][b][c];'
        f'[a]rubberband=pitch={calm_pitch}:formant=preserved,lowpass=f=6500,volume=1.0[calm];'
        '[b]highpass=f=2500,volume=0.5,aphaser=type=t:speed=0.3:decay=0.5[breath];'
        f"[c]rubberband=pitch={growl_pitch}:formant=shifted,volume=6,asoftclip=type=tanh,tremolo=f=38:d=0.55,lowpass=f=2200,"
        f"volume='{growl_from}+{growl_to - growl_from:.2f}*min(1,t/{max(0.1, d * ramp):.2f})':eval=frame[growl];"
        '[calm][breath][growl]amix=inputs=3:normalize=0,asplit=2[dry][wet0];'
        '[wet0][1:a]afir=dry=0:wet=1[wet];'
        '[dry][wet]amix=inputs=2:weights=1 0.55:normalize=0,'
        'equalizer=f=90:t=q:w=1:g=4,acompressor=threshold=0.12:ratio=3:attack=10:release=200[out]'
    )
    ff([raw, f'{tmp}/ir_temple.wav'], g, out)
    return out


def render_monk(v, text, tmp):
    raw = f'{tmp}/raw.wav'
    tts(v, 'en_GB-alan-medium', text, raw, length=1.45, noise=0.45, noise_w=0.5)
    return demon_chain(raw, tmp)


_HE = {}
HE_DEMON = dict(growl_from=0.25, growl_to=0.7, ramp=0.6, calm_pitch=0.9, growl_pitch=0.52, pad=1.6)
# Phonemes for lines whose gender joke the diacritizer cannot guess (it picks one gender).
HE_PHONEMES = {
    'נגמר לְךָ... סליחה, לָךְ... נו, למישהו פה נגמר הסוס.': 'niɡmˈaʁ leχˈa... sliχˈa, lˈaχ... nˈu, lemiʃˈehu pˈo niɡmˈaʁ hasˈus.',
    'מה זה, התחפשת למלכה אסתר?': 'mˈa zˈe, hitχapˈasta... hitχapˈast... lamalkˈa ʔestˈeʁ?',
}
def tts_he(v, text, out, length=1.4):
    """Hebrew: Phonikud diacritizer + phonemizer (open source, offline) -> Phonikud Piper model."""
    import soundfile as sf
    from phonikud_onnx import Phonikud
    from phonikud import phonemize
    from piper_onnx import Piper
    if not _HE:
        _HE['dia'] = Phonikud(os.path.join(v, 'phonikud-1.0.int8.onnx'))
        _HE['tts'] = Piper(os.path.join(v, 'michael.onnx'), os.path.join(v, 'model.config.json'))
    ph = HE_PHONEMES.get(text) or phonemize(_HE['dia'].add_diacritics(text))
    x, sr = _HE['tts'].create(ph, is_phonemes=True, length_scale=length, noise_scale=0.6, noise_w=0.7)
    sf.write(out, x, sr)
    return ph


def render_hebrew(v, text, tmp):
    raw = f'{tmp}/raw.wav'
    tts_he(v, text, raw)
    # lighter than the monk so the words survive (checked with Whisper): growl from a quarter to 0.7
    return demon_chain(raw, tmp, **HE_DEMON)


def render_announcer(v, text, tmp):
    raw = f'{tmp}/raw.wav'
    long_vowels = len(text) < 14
    tts(v, 'en_US-ryan-high', text, raw, length=1.35 if long_vowels else 1.12, noise=0.6, noise_w=0.9)
    g = (
        f'[0:a]aresample={SR},apad=pad_dur=1.6,rubberband=pitch=0.92:formant=preserved,'
        'highpass=f=90,equalizer=f=180:t=q:w=1:g=3,equalizer=f=3200:t=q:w=1.2:g=5,'
        'acompressor=threshold=0.08:ratio=8:attack=3:release=120:makeup=4,volume=1.6,asoftclip=type=atan,asplit=2[dry][w];'
        '[w]aecho=0.8:0.6:110|240:0.32|0.18,asplit=2[slap][w2];'
        '[w2][1:a]afir=dry=0:wet=1[hall];'
        '[dry][slap][hall]amix=inputs=3:weights=1 0.45 0.5:normalize=0[out]'
    )
    ff([raw, f'{tmp}/ir_stadium.wav'], g, f'{tmp}/mix.wav')
    return f'{tmp}/mix.wav'


def render_choir(v, text, tmp):
    parts = []
    for i, (model, ln) in enumerate([('en_GB-jenny_dioco-medium', 1.25), ('en_US-joe-medium', 1.3), ('en_GB-northern_english_male-medium', 1.2)]):
        p = f'{tmp}/raw{i}.wav'; tts(v, model, text, p, length=ln, noise=0.7, noise_w=0.9); parts.append(p)
    # three voices (one pitched up, two down), whisper-thin, smeared together, plus a reversed
    # reverb swell that breathes in BEFORE the words: the shadow inhales, then speaks.
    g = (
        f'[0:a]aresample={SR},rubberband=pitch=1.12:formant=shifted,adelay=0[v0];'
        f'[1:a]aresample={SR},rubberband=pitch=0.7:formant=shifted,adelay=35[v1];'
        f'[2:a]aresample={SR},rubberband=pitch=0.84:formant=preserved,adelay=70[v2];'
        '[v0][v1][v2]amix=inputs=3:normalize=0:duration=longest,highpass=f=220,'
        'chorus=0.6:0.8:40|55:0.4|0.35:0.3|0.4:2|1.5,equalizer=f=4500:t=q:w=1:g=5,'
        'adelay=1400,apad=pad_dur=1.8,asplit=3[dry][pre][post];'
        '[pre]areverse[pr];[pr][3:a]afir=dry=0:wet=1,areverse,volume=0.9[swell];'
        '[post][4:a]afir=dry=0:wet=1[tail];'
        '[dry][swell][tail]amix=inputs=3:weights=1 0.9 0.4:normalize=0,'
        'aphaser=type=s:speed=0.15:decay=0.4,'
        "silenceremove=start_periods=1:start_threshold=-60dB[out]"
    )
    ff(parts + [f'{tmp}/ir_void.wav', f'{tmp}/ir_void.wav'], g, f'{tmp}/mix.wav')
    return f'{tmp}/mix.wav'


def render_arcade(v, text, tmp):
    raw = f'{tmp}/raw.wav'
    tts(v, 'en_US-ryan-high', text, raw, length=1.2, noise=0.3, noise_w=0.3)
    g = (
        f'[0:a]aresample=8000,aresample={SR},apad=pad_dur=0.9,rubberband=pitch=1.06:formant=preserved,'
        'acrusher=bits=5:samples=3:mix=0.85:mode=lin,highpass=f=350,lowpass=f=3400,'
        'equalizer=f=1200:t=q:w=1:g=6,volume=2,asoftclip=type=hard,'
        'aecho=0.7:0.5:60|120:0.3|0.15[out]'
    )
    ff([raw], g, f'{tmp}/mix.wav')
    return f'{tmp}/mix.wav'


def laugh_abyss(v, tmp):
    raw = f'{tmp}/raw.wav'
    tts(v, 'en_GB-northern_english_male-medium', 'huh. huh. huh, huh. ha ha ha ha ha. ha ha ha ha!', raw, length=1.35, noise=0.8, noise_w=1.0)
    m = demon_chain(raw, tmp, out=f'{tmp}/m1.wav', growl_from=0.3, growl_to=1.5, ramp=0.7, calm_pitch=0.72, growl_pitch=0.42)
    ff([m], f"[0:a]volume='0.35+0.75*min(1,t/{duration(m) * 0.6:.2f})':eval=frame,equalizer=f=60:t=q:w=1:g=4[out]", f'{tmp}/mix.wav')
    return f'{tmp}/mix.wav'


def laugh_villain(v, tmp):
    raw = f'{tmp}/raw.wav'
    tts(v, 'en_US-joe-medium', 'heh. heh. heh heh heh. ha ha ha. ha ha ha ha ha ha!', raw, length=1.4, noise=0.8, noise_w=1.0)
    d = duration(raw)
    g = (f"[0:a]aresample={SR},apad=pad_dur=1.6,rubberband=pitch=0.8:formant=shifted,equalizer=f=200:t=q:w=1:g=4,"
         "acompressor=threshold=0.1:ratio=6:attack=5:release=150:makeup=3,"
         f"volume='0.25+0.9*min(1,t/{d * 0.8:.2f})':eval=frame,asplit=2[d][w];[w][1:a]afir=dry=0:wet=1[r];"
         "[d][r]amix=inputs=2:weights=1 0.5:normalize=0,aecho=0.8:0.5:180:0.25[out]")
    ff([raw, f'{tmp}/ir_temple.wav'], g, f'{tmp}/mix.wav')
    return f'{tmp}/mix.wav'


def laugh_legion(v, tmp):
    parts = []
    voices = [('en_GB-alan-medium', 0.5, 0), ('en_US-joe-medium', 0.68, 110), ('en_GB-northern_english_male-medium', 0.84, 230),
              ('en_GB-jenny_dioco-medium', 1.18, 340), ('en_US-ryan-high', 0.6, 470)]
    for i, (m, _, _) in enumerate(voices):
        p = f'{tmp}/raw{i}.wav'; tts(v, m, 'ha ha ha ha. ha ha ha ha ha ha.', p, length=1.15 + 0.07 * i, noise=0.9, noise_w=1.0); parts.append(p)
    chains = ''.join(f'[{i}:a]aresample={SR},rubberband=pitch={pt}:formant=shifted,adelay={dl},volume={0.9 if pt < 1 else 0.55}[v{i}];'
                     for i, (_, pt, dl) in enumerate(voices))
    g = (chains + ''.join(f'[v{i}]' for i in range(len(voices))) +
         f'amix=inputs={len(voices)}:normalize=0:duration=longest,apad=pad_dur=2.2,'
         'chorus=0.6:0.8:30|47:0.4|0.3:0.25|0.4:1.2|1.8,asplit=2[d][w];'
         f'[w][{len(voices)}:a]afir=dry=0:wet=1[r];[d][r]amix=inputs=2:weights=1 0.8:normalize=0,'
         'aecho=0.8:0.6:300|520:0.3|0.2[out]')
    ff(parts + [f'{tmp}/ir_void.wav'], g, f'{tmp}/mix.wav')
    return f'{tmp}/mix.wav'


def laugh_glitch(v, tmp):
    raw = f'{tmp}/raw.wav'
    tts(v, 'en_US-ryan-high', 'ha ha ha ha ha ha ha ha ha.', raw, length=1.05, noise=0.7, noise_w=0.9)
    ff([raw], f'[0:a]aresample={SR},rubberband=pitch=0.7:formant=shifted[out]', f'{tmp}/p.wav')
    import soundfile as sf
    x, sr = sf.read(f'{tmp}/p.wav'); rng = np.random.default_rng(11)
    g = int(0.055 * sr); out = []; i = 0
    while i < len(x):  # granular stutter: repeat, reverse or drop grains like a corrupted buffer
        grain = x[i:i + g]; r = rng.random()
        if r < 0.22: out += [grain] * int(rng.integers(2, 5))
        elif r < 0.32: out.append(grain[::-1])
        elif r < 0.38: out.append(grain[::2])
        else: out.append(grain)
        i += g
    y = np.concatenate(out + [np.zeros(int(0.6 * sr))]); sf.write(f'{tmp}/s.wav', y, sr)
    g2 = ('[0:a]acrusher=bits=4:samples=6:mix=0.7:mode=lin,afreqshift=shift=-90,highpass=f=150,'
          'equalizer=f=1500:t=q:w=1:g=5,volume=1.5,asoftclip=type=hard,aecho=0.6:0.5:45|90:0.4|0.2[out]')
    ff([f'{tmp}/s.wav'], g2, f'{tmp}/mix.wav')
    return f'{tmp}/mix.wav'


def laugh_whisper_roar(v, tmp):
    a = f'{tmp}/raw_a.wav'; b = f'{tmp}/raw_b.wav'
    tts(v, 'en_GB-jenny_dioco-medium', 'hee hee hee. hee hee hee hee.', a, length=1.2, noise=0.8, noise_w=1.0)
    tts(v, 'en_GB-alan-medium', 'HAAAAA ha ha ha!', b, length=1.5, noise=0.8, noise_w=1.0)
    ff([a, f'{tmp}/ir_void.wav'], f'[0:a]aresample={SR},rubberband=pitch=1.15:formant=preserved,highpass=f=1200,volume=0.6,apad=pad_dur=0.4,'
       'asplit=2[d][w];[w][1:a]afir=dry=0:wet=1[r];[d][r]amix=inputs=2:weights=1 0.9:normalize=0[out]', f'{tmp}/wa.wav')
    roar = demon_chain(b, tmp, out=f'{tmp}/wb.wav', growl_from=1.2, growl_to=1.6, ramp=0.3, calm_pitch=0.8, growl_pitch=0.45)
    ff([f'{tmp}/wa.wav', roar], '[1:a]volume=1.4[r];[0:a][r]concat=n=2:v=0:a=1[out]', f'{tmp}/mix.wav')
    return f'{tmp}/mix.wav'


def laugh_chuckle(v, tmp):
    raw = f'{tmp}/raw.wav'
    tts(v, 'en_GB-northern_english_male-medium', 'heh heh heh.', raw, length=1.2, noise=0.8, noise_w=1.0)
    return demon_chain(raw, tmp, growl_from=0.7, growl_to=0.9, ramp=0.5, calm_pitch=0.78, growl_pitch=0.5, pad=1.0)


# (id, name, when it plays, one-line description, renderer)
LAUGHS = [
    ('abyss', 'Abyss rumble', 'Fight start, after "Round one"', 'Low "huh... huh..." from deep in the chest that builds into a full sub-octave demon laugh.', laugh_abyss),
    ('villain', 'Villain crescendo', 'Player loses / time out', 'Slow "heh... heh..." that keeps climbing into a theatrical HA HA HA with a temple echo.', laugh_villain),
    ('legion', 'Legion', 'Shadow rises again in a new round', 'Five voices from an octave down to a child-high giggle laugh a beat apart, smeared into one crowd of shadows.', laugh_legion),
    ('glitch', 'Corrupted signal', 'Shadow regains HP / defeat screen', 'A laugh chewed by a broken buffer: stutters, reversed grains, 4-bit crush and a frequency shift.', laugh_glitch),
    ('whisper_roar', 'Giggle to roar', 'Idle mid-fight (long pause)', 'A thin whispered giggle right behind you, then a sudden demon roar-laugh.', laugh_whisper_roar),
    ('chuckle', 'Taunt chuckle', 'Between punches, after a weak jab', 'Short dry "heh heh heh" low in the throat, under a second long, safe to repeat.', laugh_chuckle),
]

RENDER = {'monk': render_monk, 'hebrew': render_hebrew, 'announcer': render_announcer, 'choir': render_choir, 'arcade': render_arcade}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--voices', required=True)
    ap.add_argument('--he-voices', default=None, help='dir with phonikud-1.0.int8.onnx, michael.onnx, model.config.json')
    ap.add_argument('--only', default=None)
    a = ap.parse_args()
    man_path = os.path.join(OUT, 'manifest.json')
    manifest = json.load(open(man_path)) if os.path.exists(man_path) else {}
    with tempfile.TemporaryDirectory() as tmp:
        make_ir(f'{tmp}/ir_temple.wav', 3.5, 1.6, 2500, predelay=0.03, seed=3)
        make_ir(f'{tmp}/ir_stadium.wav', 2.6, 2.4, 5000, predelay=0.04, seed=5)
        make_ir(f'{tmp}/ir_void.wav', 2.2, 2.0, 3200, seed=7)
        for ch, lines in LINES.items():
            if (a.only and ch != a.only) or (ch == 'hebrew' and not a.he_voices):
                continue
            os.makedirs(os.path.join(OUT, ch), exist_ok=True)
            manifest[ch] = []
            for lid, cat, text, *gloss in lines:
                mix = RENDER[ch](a.he_voices if ch == 'hebrew' else a.voices, text, tmp)
                dst = os.path.join(OUT, ch, lid + '.mp3')
                lufs, tp = finish(mix, dst)
                manifest[ch].append({'id': lid, 'cat': cat, 'text': text, 'file': f'voice/{ch}/{lid}.mp3',
                                     'dur': round(duration(dst), 2), 'lufs': lufs, 'tp': tp, **({'gloss': gloss[0]} if gloss else {})})
                print(f'{ch:9} {lid:12} {lufs:6.1f} LUFS  {tp:5.1f} dBTP  {text}')
        if not a.only or a.only == 'laugh':
            os.makedirs(os.path.join(OUT, 'laugh'), exist_ok=True)
            manifest['laugh'] = []
            for lid, name, when, desc, fn in LAUGHS:
                dst = os.path.join(OUT, 'laugh', lid + '.mp3')
                lufs, tp = finish(fn(a.voices, tmp), dst)
                manifest['laugh'].append({'id': lid, 'name': name, 'when': when, 'desc': desc, 'file': f'voice/laugh/{lid}.mp3',
                                          'dur': round(duration(dst), 2), 'lufs': lufs, 'tp': tp})
                print(f'laugh     {lid:12} {lufs:6.1f} LUFS  {tp:5.1f} dBTP  {name}')
    json.dump(manifest, open(man_path, 'w'), indent=1, ensure_ascii=False)


if __name__ == '__main__':
    main()
