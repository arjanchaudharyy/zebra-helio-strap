"""Health watch: rules over the data downloaded from the strap.

Every rule compares the person only with their own recent normal. Each new alert is written to
`alerts.json` (read by Zebra) and returned so the caller can buzz the strap once. An alert id is
remembered in `alerts-state.json`, so the same event never buzzes twice.
"""
from __future__ import annotations

import datetime as dt
import json
import statistics as st
from collections import defaultdict
from pathlib import Path
from typing import Optional

# Rules, kept in one place so they are easy to read and tune.
HIGH_HR_STILL_BPM_OVER_REST = 45      # HR this far above resting while not moving...
HIGH_HR_STILL_MINUTES = 10            # ...for at least this long
STRESS_HIGH = 80
STRESS_HIGH_MINUTES = 30
SPO2_LOW = 90
SPO2_LOW_READINGS = 3                 # low readings in one night
RHR_RISE_BPM = 7
FEVER_TEMP_RISE_C = 0.5
FEVER_RHR_RISE_BPM = 3
FEVER_HRV_DROP = 0.15                 # 15 % below normal
LOOKBACK_HOURS = 24


def _load(path: Path) -> list[dict]:
    try:
        return [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
    except FileNotFoundError:
        return []


def _night_key(ts: int) -> str:
    """A night belongs to the date you wake up: readings from 18:00 count toward the next day."""
    d = dt.datetime.fromtimestamp(ts)
    if d.hour >= 18:
        d += dt.timedelta(days=1)
    return d.strftime("%Y-%m-%d")


def _is_night(ts: int) -> bool:
    h = dt.datetime.fromtimestamp(ts).hour
    return h >= 23 or h < 7


def _fmt(ts: int) -> str:
    return dt.datetime.fromtimestamp(ts).strftime("%-I:%M %p").lower()


def nightly(out: Path) -> dict[str, dict]:
    """Per night: median skin temperature (night hours), resting HR, mean HRV."""
    temps: dict[str, list] = defaultdict(list)
    for r in _load(out / "46_skin_temperature.jsonl"):
        v = r.get("skin_centi_celsius")
        if isinstance(v, (int, float)) and _is_night(r["timestamp"]) and 2500 < v < 4200:
            temps[_night_key(r["timestamp"])].append(v / 100)
    hrv: dict[str, list] = defaultdict(list)
    for r in _load(out / "73_hrv_rmssd.jsonl"):
        v = r.get("rmssd")
        if isinstance(v, (int, float)) and 0 < v < 250:
            hrv[_night_key(r["timestamp"])].append(v)
    rhr: dict[str, float] = {}
    for r in _load(out / "58_resting_heart_rate.jsonl"):
        v = r.get("heart_rate")
        if isinstance(v, (int, float)) and 25 < v < 130:
            rhr[dt.datetime.fromtimestamp(r["timestamp"]).strftime("%Y-%m-%d")] = v
    nights = sorted(set(temps) | set(hrv) | set(rhr))
    return {
        n: {
            "temp": round(st.median(temps[n]), 2) if len(temps[n]) >= 60 else None,
            "hrv": round(st.mean(hrv[n]), 1) if hrv[n] else None,
            "rhr": rhr.get(n),
        }
        for n in nights
    }


BASELINE_NIGHTS = 7                   # nights of your normal needed before the illness warning judges


def _baseline(nights: dict[str, dict], key: str, before: str, need: int = 3) -> Optional[float]:
    vals = [v[key] for n, v in nights.items() if n < before and v.get(key) is not None][-21:]
    return st.median(vals) if len(vals) >= need else None


def illness_status(out: Path) -> dict:
    """Fever / illness early warning for the most recent night."""
    nights = nightly(out)
    if not nights:
        return {"state": "no_data", "text": "No night data from the strap yet."}
    last = max(nights)
    cur = nights[last]
    base_t = _baseline(nights, "temp", last, BASELINE_NIGHTS)
    base_r = _baseline(nights, "rhr", last, BASELINE_NIGHTS)
    base_h = _baseline(nights, "hrv", last, BASELINE_NIGHTS)
    have = sum(1 for n, v in nights.items() if n < last and v.get("temp") is not None)
    if base_t is None or cur.get("temp") is None:
        return {"state": "learning", "night": last, "nightsWithTemp": have, "nightsNeeded": BASELINE_NIGHTS,
                "text": f"Needs more data: {have} of {BASELINE_NIGHTS} nights of your normal so far. Keep the strap on at night."}
    dt_c = round(cur["temp"] - base_t, 2)
    rhr_up = base_r is not None and cur.get("rhr") is not None and cur["rhr"] - base_r >= FEVER_RHR_RISE_BPM
    hrv_down = base_h is not None and cur.get("hrv") is not None and cur["hrv"] <= base_h * (1 - FEVER_HRV_DROP)
    state = "normal"
    if dt_c >= FEVER_TEMP_RISE_C and (rhr_up or hrv_down):
        state = "warning"
    elif dt_c >= FEVER_TEMP_RISE_C or (rhr_up and hrv_down):
        state = "watch"
    signs = []
    if dt_c >= FEVER_TEMP_RISE_C:
        signs.append(f"skin temperature +{dt_c:.1f} °C")
    if rhr_up:
        signs.append(f"resting HR +{cur['rhr'] - base_r:.0f} bpm")
    if hrv_down:
        signs.append(f"HRV {100 * (1 - cur['hrv'] / base_h):.0f}% lower")
    text = {
        "normal": f"Night temperature normal ({dt_c:+.1f} °C vs usual).",
        "watch": "One sign is off: " + ", ".join(signs) + ". Rest and drink water.",
        "warning": "Possible illness coming: " + ", ".join(signs) + ". Take it easy today.",
    }[state]
    return {"state": state, "night": last, "tempDelta": dt_c, "temp": cur["temp"], "baseTemp": round(base_t, 2),
            "rhrUp": rhr_up, "hrvDown": hrv_down, "text": text}


def _runs(points: list[tuple[int, bool]], min_len_s: int) -> list[tuple[int, int]]:
    """Time spans where the flag stays true for at least min_len_s (gaps over 3 min break a run)."""
    runs, start, prev = [], None, None
    for ts, flag in points:
        if flag and start is not None and prev is not None and ts - prev <= 180:
            prev = ts
            continue
        if start is not None and prev - start >= min_len_s:
            runs.append((start, prev))
        start, prev = (ts, ts) if flag else (None, None)
    if start is not None and prev - start >= min_len_s:
        runs.append((start, prev))
    return runs


def check(out: Path, now: Optional[int] = None) -> list[dict]:
    """Evaluate all rules over the last day. Returns NEW alerts (not seen before)."""
    now = now or int(dt.datetime.now().timestamp())
    since = now - LOOKBACK_HOURS * 3600
    alerts: list[dict] = []

    nights = nightly(out)
    rest = _baseline(nights, "rhr", "9999") or 65

    # Steps per minute, to know when the person is still.
    moving = {}
    for r in _load(out / "01_activity.jsonl"):
        if r["timestamp"] >= since - 600:
            moving[r["timestamp"] // 60] = (r.get("steps") or 0) > 5
    # 1. High heart rate while still.
    per_min: dict[int, list] = defaultdict(list)
    for r in _load(out / "87_sec_hr_v2.jsonl"):
        if r["timestamp"] >= since and r.get("heart_rate"):
            per_min[r["timestamp"] // 60].append(r["heart_rate"])
    pts = [(m * 60, st.mean(v) >= rest + HIGH_HR_STILL_BPM_OVER_REST and not moving.get(m, False)) for m, v in sorted(per_min.items())]
    for a, b in _runs(pts, HIGH_HR_STILL_MINUTES * 60):
        peak = max(max(per_min[m]) for m in range(a // 60, b // 60 + 1) if m in per_min)
        alerts.append({"id": f"hr-still-{a}", "kind": "high_hr", "at": a,
                       "text": f"Heart rate stayed high ({peak} bpm peak) for {round((b - a) / 60)} min while you were still, from {_fmt(a)}."})

    # 2. High stress for a long time.
    pts = [(r["timestamp"], (r.get("stress") or 0) >= STRESS_HIGH and r.get("stress", 0) <= 100)
           for r in _load(out / "19_stress_all_day.jsonl") if r["timestamp"] >= since]
    for a, b in _runs(sorted(pts), STRESS_HIGH_MINUTES * 60):
        alerts.append({"id": f"stress-{a}", "kind": "stress", "at": a,
                       "text": f"Stress stayed high for {round((b - a) / 60)} min from {_fmt(a)}. Try 3 slow breaths."})

    # 3. Low blood oxygen during sleep.
    lows = [r for r in _load(out / "37_spo2_single.jsonl")
            if r["timestamp"] >= since and isinstance(r.get("spo2"), int) and 70 <= r["spo2"] < SPO2_LOW and _is_night(r["timestamp"])]
    if len(lows) >= SPO2_LOW_READINGS:
        n = _night_key(lows[0]["timestamp"])
        alerts.append({"id": f"spo2-{n}", "kind": "spo2", "at": lows[0]["timestamp"],
                       "text": f"Blood oxygen dipped below {SPO2_LOW}% {len(lows)} times last night (lowest {min(r['spo2'] for r in lows)}%)."})

    # 4. Resting heart rate well above usual.
    if nights:
        last = max(nights)
        base = _baseline(nights, "rhr", last)
        cur = nights[last].get("rhr")
        if base is not None and cur is not None and cur - base >= RHR_RISE_BPM:
            alerts.append({"id": f"rhr-{last}", "kind": "resting_hr", "at": now,
                           "text": f"Resting heart rate is {cur:.0f} bpm, {cur - base:.0f} above your usual. Short sleep, stress or illness can do this."})

    # 5. Fever / illness early warning.
    ill = illness_status(out)
    if ill["state"] in ("watch", "warning"):
        alerts.append({"id": f"ill-{ill['state']}-{ill['night']}", "kind": "illness", "at": now, "level": ill["state"], "text": ill["text"]})

    # Keep only new alerts; store all recent ones for Zebra.
    state_file = out / "alerts-state.json"
    try:
        seen = set(json.loads(state_file.read_text()))
    except (FileNotFoundError, ValueError):
        seen = set()
    new = [a for a in alerts if a["id"] not in seen]
    state_file.write_text(json.dumps(sorted(seen | {a["id"] for a in alerts})[-500:]))
    history_file = out / "alerts.json"
    try:
        history = json.loads(history_file.read_text()).get("alerts", [])
    except (FileNotFoundError, ValueError, AttributeError):
        history = []
    history = (new + history)[:100]
    history_file.write_text(json.dumps({"alerts": history, "illness": ill, "checkedAt": now}))
    return new
