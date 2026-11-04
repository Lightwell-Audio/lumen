"""Writes the listening page (index.html) from the trial's summary.json.

Usage, from a folder holding media/<source>/<transform>/<letter>.mp4 (the
pack as FLAC in MP4, which the page host serves):
    python3 -I make_page.py <the trial's out dir>
"""
import html
import json
import sys

summary = json.load(open(f"{sys.argv[1]}/summary.json"))


def row(x):
    blocks = x["block_indep"]
    if blocks is None:
        independent = "—"
    elif blocks == x["n"]:
        independent = "yes"
    elif blocks == 0:
        independent = "no"
    else:
        independent = f"no <span class='dim'>({blocks} of {x['n']})</span>"
    latency = "—" if x["latency_ms"] is None else f"{x['latency_ms']:.0f} ms"
    allocs = "—" if x["allocs"] is None else ("none" if x["allocs"] == 0 else str(x["allocs"]))
    return (
        f"<tr><th scope='row'>{x['letter']}</th>"
        f"<td>{x['cpu_median'] / 10:.1f}% <span class='dim'>(max {x['cpu_max'] / 10:.1f}%)</span></td>"
        f"<td>{x['chunk_median']:.0f} ms <span class='dim'>(max {x['chunk_max']:.0f})</span></td>"
        f"<td>{latency}</td>"
        f"<td>{independent}</td>"
        f"<td>{allocs}</td>"
        f"<td>{'yes' if x['repeats'] else 'no'}</td></tr>"
    )


rows = "\n".join(row(summary[k]) for k in "ABCDE")
SOURCES = [
    ("1-drums", "Drum loop", "A 108 BPM breakbeat, two overhead microphones, 8.9 s."),
    ("2-voice", "Voice", "A solo singer, a public-domain hymn, 8 s."),
    ("3-full-mix", "Full mix", "A military band playing a march: brass, woodwinds and percussion, 8 s."),
]
TRANSFORMS = [
    ("tempo-0.75", "Slower: 75% speed", "Plays a quarter slower"),
    ("tempo-1.5", "Faster: 150% speed", "Plays half again as fast"),
    ("pitch-minus-5", "Down 5 semitones", "Same speed"),
    ("pitch-plus-3", "Up 3 semitones", "Same speed"),
]


def player(path, label, cls=""):
    return (
        f'<figure class="pl {cls}"><figcaption>{html.escape(label)}</figcaption>'
        f'<audio controls preload="none" src="{path}" aria-label="{html.escape(label)}"></audio></figure>'
    )


sections = []
for sid, title, note in SOURCES:
    parts = [f'<section class="src" aria-labelledby="h-{sid}"><h2 id="h-{sid}">{title}</h2><p class="note">{note}</p>']
    parts.append('<div class="row"><div class="rowhead"><b>Original</b></div><div class="grid">' + player(f"media/{sid}/original.mp4", "Original", "orig") + "</div></div>")
    for tid, tt, tn in TRANSFORMS:
        players = "".join(player(f"media/{sid}/{tid}/{k}.mp4", k) for k in "ABCDE")
        parts.append(f'<div class="row"><div class="rowhead"><b>{tt}</b><span>{tn}</span></div><div class="grid">{players}</div></div>')
    whole = "".join(player(f"media/{sid}/whole-tempo-0.9/{k}.mp4", f"{k} straight") for k in "ABCDE")
    chunk = "".join(player(f"media/{sid}/chunked-tempo-0.9/{k}.mp4", f"{k} prepared") for k in "ABCDE")
    parts.append(
        '<div class="row"><div class="rowhead"><b>Prepared ahead, 90% speed</b><span>Top: rendered straight through. Bottom: rendered in 85 ms pieces, each from its own lead-in, as plan E A would. Listen for flutter or roughness in the bottom row.</span></div>'
        f'<div class="grid">{whole}</div><div class="grid">{chunk}</div></div>'
    )
    parts.append("</section>")
    sections.append("".join(parts))

page = f"""<title>Lumen Stretch Trial</title>
<meta name="claude:viewport-fit-sides" content="cover">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600&family=Martian+Mono:wght@400;500&family=Unbounded:wght@500;600&display=swap">
<style>
/* Layout: a listening sheet on Lumen's ground: the measurements first, then one block per source with rows of A to E players. */
:root {{
  --ground:#111316; --app:#17191D; --raised:#23262D; --well:#141619; --border:#2E323A; --border_strong:#3C414B;
  --text:#E6E8EC; --text_secondary:#9AA0AC; --text_hint:#6E7480; --accent:#4FA3FF; --accent_muted:#243A55;
  --f-ui:"Inter",system-ui,-apple-system,"Segoe UI",sans-serif; --f-title:"Unbounded","Inter",system-ui,sans-serif;
  --f-mono:"Martian Mono",ui-monospace,Consolas,monospace; color-scheme: dark;
}}
@media (prefers-color-scheme: light) {{ :root:not([data-theme="dark"]) {{
  --ground:#E7E9ED; --app:#F1F2F4; --raised:#E9EBEE; --well:#D6DAE0; --border:#DADDE2; --border_strong:#C3C8CF;
  --text:#1C1F24; --text_secondary:#5A6170; --text_hint:#878D98; --accent:#1F6FD1; --accent_muted:#D5E5FA; color-scheme: light; }} }}
:root[data-theme="light"] {{
  --ground:#E7E9ED; --app:#F1F2F4; --raised:#E9EBEE; --well:#D6DAE0; --border:#DADDE2; --border_strong:#C3C8CF;
  --text:#1C1F24; --text_secondary:#5A6170; --text_hint:#878D98; --accent:#1F6FD1; --accent_muted:#D5E5FA; color-scheme: light; }}
body {{ background: var(--ground); color: var(--text); font: 14px/1.55 var(--f-ui); }}
.page {{ max-width: 1180px; margin: 0 auto; padding-block: 28px 64px;
  padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px)); }}
h1 {{ font: 600 26px var(--f-title); margin: 0; text-wrap: balance; }}
h2 {{ font: 500 18px var(--f-title); margin: 0; }}
.lede {{ color: var(--text_secondary); max-width: 72ch; }}
.ask {{ background: var(--accent_muted); border: 1px solid var(--accent); border-radius: 10px; padding: 12px 16px; max-width: 80ch; }}
.ask b {{ color: var(--text); }}
.tablewrap {{ overflow-x: auto; margin-top: 12px; }}
table {{ border-collapse: collapse; min-width: 640px; width: 100%; font-variant-numeric: tabular-nums; }}
th, td {{ text-align: left; padding: 8px 12px; border-bottom: 1px solid var(--border); }}
thead th {{ color: var(--text_hint); font-weight: 600; font-size: 11px; letter-spacing: .06em; text-transform: uppercase; }}
tbody th {{ font: 500 14px var(--f-mono); }}
.dim {{ color: var(--text_hint); }}
.src {{ margin-top: 36px; background: var(--app); border: 1px solid var(--border); border-radius: 12px; padding: 18px 18px 8px; }}
.note {{ color: var(--text_secondary); margin: 2px 0 12px; }}
.row {{ border-top: 1px solid var(--border); padding: 12px 0; display: grid; gap: 8px; }}
.rowhead {{ display: flex; flex-wrap: wrap; gap: 4px 12px; align-items: baseline; }}
.rowhead span {{ color: var(--text_hint); font-size: 12.5px; min-width: 0; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 200px), 1fr)); gap: 8px; }}
figure.pl {{ margin: 0; background: var(--raised); border: 1px solid var(--border); border-radius: 8px; padding: 6px 8px; min-width: 0; }}
figure.pl.playing {{ border-color: var(--accent); }}
figcaption {{ font: 500 13px var(--f-mono); margin-bottom: 2px; }}
audio {{ width: 100%; height: 32px; display: block; }}
.credits {{ color: var(--text_hint); font-size: 12.5px; margin-top: 32px; max-width: 90ch; }}
.credits a {{ color: var(--text_secondary); }}
</style>
<div class="page">
<h1>Lumen Stretch Trial</h1>
<p class="lede">Milestone 9.5, slice S0 (choice T). Four time-stretch libraries, one of them in two engine settings, are lettered A to E. The letters stay the same on every row, so you can follow one you like. The names are hidden until you choose. Every file is lossless, 48 kHz, 16-bit, and loudness-matched to its original, so no letter wins by being louder.</p>
<p class="ask"><b>What to send back:</b> your favourite letter overall, any letter you would rule out, and whether the bottom "prepared" rows sound different from the top ones. Headphones help.</p>

<h2 style="margin-top:28px">Measurements</h2>
<p class="lede">One core of a 2.8 GHz server, one stereo clip at 48 kHz, 6 transforms × 3 sources. CPU is the share of one core for one clip playing. "First piece" is the work to prepare 85 ms of audio from a 340 ms lead-in after a jump: how long a click waits before the stretched clip can sound, before any other delay.</p>
<div class="tablewrap"><table>
<thead><tr><th scope="col">Letter</th><th scope="col">CPU per clip</th><th scope="col">First piece</th><th scope="col">Own latency</th><th scope="col">Same output at any block size</th><th scope="col">Allocations while playing</th><th scope="col">Repeats bit for bit</th></tr></thead>
<tbody>
{rows}
</tbody></table></div>
<p class="dim" style="font-size:12.5px">D renders only a whole buffer at a time through the API used here, so its block-size, latency and allocation columns were not measured. "Own latency" is extra lead-in a clip needs; under plan E A it is covered by the lead-in, not heard as delay.</p>

{''.join(sections)}

<p class="credits">Sources: drum loop "108 bpm breakbeat" by EatMe (CC0, archive.org/details/108_bpm_breakbeat_to_use); voice from "Public Domain Hymns 01", LibriVox (public domain, archive.org/details/publicdomainhymns_01_1110_librivox), 48 to 56 s of track 4; full mix "Liberty Bell March" by the U.S. Army Field Band (public domain, archive.org/details/LibertyBellMarch_747), 30 to 38 s. The trial program is <code>tools/stretch-trial</code> on branch <code>m9.5-s0-stretch-trial</code>.</p>
</div>
<script>
// One player at a time, and the playing one is outlined.
document.addEventListener("play", e => {{
  for (const a of document.querySelectorAll("audio")) if (a !== e.target) a.pause();
  for (const f of document.querySelectorAll("figure.pl")) f.classList.toggle("playing", f.contains(e.target));
}}, true);
</script>
"""
open("index.html", "w").write(page)
print(len(page))
