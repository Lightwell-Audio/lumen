"""Summarises the trial's results.json by letter into summary.json: the
median and worst of each measurement, and null where a candidate's API did
not allow it to be measured (D renders a whole buffer).

Usage: python3 -I summarise.py <the trial's out dir>
"""
import json
import statistics as st
import sys

OUT = sys.argv[1]
results = json.load(open(f"{OUT}/results.json"))
sustained = json.load(open(f"{OUT}/sustained.json"))
by_letter = {}
for row in results:
    by_letter.setdefault(row["letter"], []).append(row)
summary = {}
for letter, rows in sorted(by_letter.items()):
    streams = "whole-buffer" not in rows[0]["candidate"]
    cpu = [r["ms_per_second"] for r in rows]
    chunk = [r["first_chunk_ms"] for r in rows]
    summary[letter] = {
        "letter": letter,
        "candidate": rows[0]["candidate"],
        "cpu_median": round(st.median(cpu), 1),
        "cpu_max": round(max(cpu), 1),
        "chunk_median": round(st.median(chunk), 1),
        "chunk_max": round(max(chunk), 1),
        "latency_ms": round(st.median(r["latency_ms"] for r in rows), 1) if streams else None,
        "allocs": max(r["steady_allocs"] for r in rows) if streams else None,
        "repeats": all(r["repeats_bit_for_bit"] for r in rows),
        "block_indep": sum(r["block_size_independent"] for r in rows) if streams else None,
        "n": len(rows),
        "ea_85ms": round(st.median(r["ms_per_second_85ms_pieces"] for r in sustained if r["letter"] == letter), 1),
        "ea_2s": round(st.median(r["ms_per_second_2s_pieces"] for r in sustained if r["letter"] == letter), 1),
    }
json.dump(summary, open(f"{OUT}/summary.json", "w"), indent=1)
for s in summary.values():
    print(s)
