import json, datetime as dt
from pathlib import Path
from zebrastrap import watch

def write(d: Path, name, rows):
    (d / name).write_text("".join(json.dumps(r) + "\n" for r in rows))

def night_ts(day_offset, hour):
    base = dt.datetime(2026, 11, 10, hour, 0) + dt.timedelta(days=day_offset)
    return int(base.timestamp())

def base_nights(d, temps, rhrs, hrvs):
    temp_rows, rhr_rows, hrv_rows = [], [], []
    for i, (t, r, h) in enumerate(zip(temps, rhrs, hrvs)):
        start = night_ts(i, 1)
        temp_rows += [{"timestamp": start + m * 60, "skin_centi_celsius": int(t * 100)} for m in range(120)]
        rhr_rows.append({"timestamp": night_ts(i, 8), "heart_rate": r})
        hrv_rows.append({"timestamp": start + 3600, "rmssd": h})
    write(d, "46_skin_temperature.jsonl", temp_rows)
    write(d, "58_resting_heart_rate.jsonl", rhr_rows)
    write(d, "73_hrv_rmssd.jsonl", hrv_rows)

def test_fever_warning_needs_temp_and_second_sign(tmp_path):
    base_nights(tmp_path, [34.0, 34.1, 33.9, 34.0, 34.0, 34.1, 33.9, 34.8], [62, 63, 62, 62, 62, 63, 62, 67], [50, 52, 49, 51, 50, 52, 49, 40])
    s = watch.illness_status(tmp_path)
    assert s["state"] == "warning" and s["tempDelta"] >= 0.5

def test_fever_watch_on_temp_only(tmp_path):
    base_nights(tmp_path, [34.0, 34.1, 33.9, 34.0, 34.0, 34.1, 33.9, 34.7], [62, 63, 62, 62, 62, 63, 62, 62], [50, 52, 49, 51, 50, 52, 49, 50])
    assert watch.illness_status(tmp_path)["state"] == "watch"

def test_normal_night(tmp_path):
    base_nights(tmp_path, [34.0, 34.1, 33.9, 34.0, 34.0, 34.1, 33.9, 34.1], [62, 63, 62, 62, 62, 63, 62, 62], [50, 52, 49, 51, 50, 52, 49, 50])
    assert watch.illness_status(tmp_path)["state"] == "normal"

def test_high_hr_while_still_and_no_repeat(tmp_path):
    base_nights(tmp_path, [34, 34, 34, 34], [60, 60, 60, 60], [50, 50, 50, 50])
    now = int(dt.datetime(2026, 11, 20, 15, 0).timestamp())
    start = now - 3600
    write(tmp_path, "87_sec_hr_v2.jsonl", [{"timestamp": start + s, "heart_rate": 120} for s in range(0, 15 * 60)])
    write(tmp_path, "01_activity.jsonl", [{"timestamp": start + m * 60, "steps": 0} for m in range(15)])
    new = watch.check(tmp_path, now=now)
    assert [a["kind"] for a in new] == ["high_hr"]
    assert watch.check(tmp_path, now=now) == []

def test_no_high_hr_alert_while_walking(tmp_path):
    base_nights(tmp_path, [34, 34, 34, 34], [60, 60, 60, 60], [50, 50, 50, 50])
    now = int(dt.datetime(2026, 11, 20, 15, 0).timestamp())
    start = now - 3600
    write(tmp_path, "87_sec_hr_v2.jsonl", [{"timestamp": start + s, "heart_rate": 120} for s in range(0, 15 * 60)])
    write(tmp_path, "01_activity.jsonl", [{"timestamp": start + m * 60, "steps": 90} for m in range(15)])
    assert watch.check(tmp_path, now=now) == []

def test_stress_and_spo2(tmp_path):
    now = int(dt.datetime(2026, 11, 20, 9, 0).timestamp())
    write(tmp_path, "19_stress_all_day.jsonl", [{"timestamp": now - 7200 + m * 60, "stress": 85} for m in range(40)])
    write(tmp_path, "37_spo2_single.jsonl", [{"timestamp": int(dt.datetime(2026, 11, 20, 3, m).timestamp()), "spo2": 86} for m in range(4)])
    kinds = sorted(a["kind"] for a in watch.check(tmp_path, now=now))
    assert kinds == ["spo2", "stress"]


def test_needs_seven_nights(tmp_path):
    base_nights(tmp_path, [34.0, 34.1, 33.9, 34.0, 35.0], [62, 63, 62, 62, 70], [50, 52, 49, 51, 30])
    s = watch.illness_status(tmp_path)
    assert s["state"] == "learning" and s["nightsWithTemp"] == 4 and "4 of 7" in s["text"]
