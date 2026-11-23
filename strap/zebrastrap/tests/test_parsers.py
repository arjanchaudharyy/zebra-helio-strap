"""RecordParsersTest.kt for the ported types: 1, 87 (+76), 72, 73, 78, 37, 38, 39, 19, 18, 46, 56, 58, 61."""

import dataclasses
import struct
from datetime import datetime
from zoneinfo import ZoneInfo

from zebrastrap import history as h
from zebrastrap._bytes import ByteWriter

T0 = 1_760_000_000
CTX = h.ParseContext(header_time=T0)


def ok(r):
    assert isinstance(r, h.ParseSuccess), r
    return r


def fail(r):
    assert isinstance(r, h.ParseFailure), r


def w():
    return ByteWriter()


def rep(b: ByteWriter, n: int, v: int) -> ByteWriter:
    for _ in range(n):
        b.u8(v)
    return b


# ---- activity

def test_activity_mode_is_low_nibble():
    p = w().u8(0x73).u8(0).u8(0).u8(255).u8(5).u8(0).u8(0).u8(0).u8(0x70).u8(56).u8(8).u8(87).u8(5).u8(0).u8(0).u8(0)
    not_worn, worn = ok(h.parse_activity(p.to_bytes(), dataclasses.replace(CTX, activity_record_size=8))).records
    assert not_worn.mode == h.ActivityMode.NOT_WORN and not not_worn.is_worn and not_worn.heart_rate is None
    assert worn.mode == h.ActivityMode.SILENT and worn.raw_mode == 0x70 and worn.is_worn
    assert worn.steps == 8 and worn.heart_rate == 87


def test_activity_4_byte():
    p = bytes([1, 40, 12, 72, 3, 0, 0, 254, 0, 1, 0, 255, 9])
    res = ok(h.parse_activity(p, CTX))
    assert len(res.records) == 3 and res.dropped_bytes == 1
    a = res.records[0]
    assert a.timestamp == T0 and a.mode == h.ActivityMode.WALKING
    assert (a.intensity, a.steps, a.heart_rate) == (40, 12, 72) and a.sleep_extra is None
    assert res.records[1].timestamp == T0 + 60 and res.records[1].heart_rate is None
    assert res.records[1].raw_heart_rate == 254 and res.records[2].heart_rate is None
    assert res.next_cursor == T0 + 180


def test_activity_8_byte():
    p = bytes([5, 2, 0, 55, 9, 8, 7, 6])
    a = ok(h.parse_activity(p, dataclasses.replace(CTX, activity_record_size=8))).records[0]
    assert a.heart_rate == 55 and a.sleep_extra == h.ActivitySleepExtra(9, 8, 7, 6)
    assert len(ok(h.parse_activity(p, dataclasses.replace(CTX, activity_record_size=0))).records) == 2


def test_activity_empty():
    res = ok(h.parse_activity(b"", CTX))
    assert res.records == [] and res.next_cursor is None


# ---- per-second HR

def test_sec_hr_v1():
    res = ok(h.parse_second_hr(w().u32(T0).u8(80).u32(T0 + 1).u8(255).u8(1).to_bytes(), CTX))
    assert res.records[0].heart_rate == 80 and res.records[1].heart_rate is None
    assert res.next_cursor == T0 + 2 and res.dropped_bytes == 1


def test_sec_hr_v2():
    p = (w().u32(T0).u16(4).u8(60).u8(61).u8(0xFF).u8(0xFF).u32(T0 + 100).u16(0)
         .u32(T0 + 200).u16(2).u8(70).u8(71).to_bytes())
    res = ok(h.parse_second_hr_v2(p, CTX))
    assert [r.timestamp for r in res.records] == [T0, T0 + 1, T0 + 200, T0 + 201]
    assert [r.heart_rate for r in res.records] == [60, 61, 70, 71]
    assert res.next_cursor == T0 + 202


def test_sec_hr_v2_only_padding_ends_the_loop():
    res = ok(h.parse_second_hr_v2(w().u32(T0).u16(3).u8(0xFF).u8(0xFF).u8(0xFF).to_bytes(), CTX))
    assert res.records == [] and res.next_cursor is None


def test_sec_hr_v2_truncated_and_duplicate():
    fail(h.parse_second_hr_v2(w().u32(T0).u16(10).u8(60).to_bytes(), CTX))
    dup = w().u32(T0).u16(2).u8(60).u8(61).u32(T0 + 1).u16(1).u8(99).to_bytes()
    assert [r.raw_heart_rate for r in ok(h.parse_second_hr_v2(dup, CTX)).records] == [60, 99]


# ---- sleep

def sleep_record(ts, main_stages, short_count=1, naps=()):
    b = (w().u32(ts).u32(ts - 3600).u8(2).u8(1).u16(-60).u16(420).u16(-70).u16(430)
         .u8(1).u8(1).u8(1).u8(58).u8(84).u8(short_count))
    for i in range(10):
        b.u16(i * 10).u16(i * 10 + 5).u16(5)
    b.u8(len(main_stages)).u8(0)
    for stages in (main_stages, naps):
        for i in range(50):
            s = stages[i] if i < len(stages) else (0, 0, 0)
            b.u16(s[0]).u16(s[1]).u8(s[2])
    b.u16(90).u16(200).u16(80).u16(30)
    out = b.to_bytes()
    assert len(out) == h.SLEEP_RESULT_SIZE
    return out


def test_sleep_result():
    stages = [(-60, 0, 7), (0, 120, 4), (120, 200, 5), (200, 300, 8), (300, 420, 9)]
    p = sleep_record(T0, stages) + sleep_record(T0 + 86400, [], short_count=0)
    res = ok(h.parse_sleep_result(p, CTX))
    assert len(res.records) == 2
    s = res.records[0]
    assert (s.timestamp, s.belong_time, s.version, s.has_main_sleep) == (T0, T0 - 3600, 2, True)
    assert (s.main_sleep_start, s.main_sleep_end, s.on_bed_time, s.off_bed_time) == (-60, 420, -70, 430)
    assert (s.average_heart_rate, s.sleep_score) == (58, 84)
    assert s.short_sleeps == [h.ShortSleep(0, 5, 5)]
    assert [x.type for x in s.main_stages] == ["AWAKE", "LIGHT", "DEEP", "REM", "UNKNOWN"]
    assert s.main_stages[4].raw_type == 9 and s.short_stages == []
    assert (s.rem_sleep_length, s.light_sleep_length, s.deep_sleep_length, s.wake_length) == (90, 200, 80, 30)
    assert s.wake_count == 1 and res.records[1].main_stages == []
    assert res.next_cursor == T0 + 86401
    trunc = ok(h.parse_sleep_result(p[:594 + 100], CTX))
    assert len(trunc.records) == 1 and trunc.dropped_bytes == 100


def test_sleep_times_are_minutes_from_the_day_before():
    zone = ZoneInfo("Asia/Dubai")
    midnight = int(datetime(2026, 10, 16, tzinfo=zone).timestamp())
    raw = bytearray(sleep_record(midnight, [(1410, 1500, 4), (1500, 1860, 5)], naps=[(2280, 2310, 4)]))
    struct.pack_into("<I", raw, 4, midnight)
    s = ok(h.parse_sleep_result(bytes(raw), CTX)).records[0]
    assert s.day(zone) == midnight
    assert s.resolve_time(1410, zone) == midnight - 30 * 60
    assert s.resolve_time(1860, zone) == midnight + 7 * 3600
    assert [x.type for x in s.main_stages] == ["LIGHT", "DEEP"]
    assert s.short_stages == [h.SleepStage(2280, 2310, 4)]
    assert s.resolve_time(s.short_stages[0].start, zone) == midnight + 14 * 3600


# ---- SpO2

def test_spo2_legacy():
    p = w().u32(T0).u8(0x80 | 97).u32(T0 + 60).u8(70).u32(T0 + 120).u8(127).u32(T0 + 180).u8(0).to_bytes()
    r = ok(h.parse_spo2_single(p, CTX)).records
    assert r[0].spo2 == 97 and r[0].is_auto and r[0].is_plausible
    assert r[1].spo2 == 70 and not r[1].is_auto and not r[1].is_plausible
    assert not r[2].is_plausible and not r[3].is_plausible and r[0].history is None


def test_spo2_versioned():
    v = dataclasses.replace(CTX, spo2_versioned=True)
    assert ok(h.parse_spo2_single(w().u8(1).u32(T0).u8(95).to_bytes(), v)).records[0].spo2 == 95
    r2 = ok(h.parse_spo2_single(rep(w().u8(2).u32(T0).u8(0x80 | 99), 60, 96).to_bytes(), v)).records[0]
    assert r2.spo2 == 99 and r2.is_auto and len(r2.history) == 60
    fail(h.parse_spo2_single(w().u8(7).u32(T0).u8(1).to_bytes(), v))
    assert ok(h.parse_spo2_single(b"", v)).records == []


def test_spo2_version_detected_without_feature_bit():
    ts = 0x6AD13C97
    hw = rep(rep(w().u8(2).u32(ts).u8(99), 60, 0).u32(ts + 600).u8(98), 60, 0).to_bytes()
    assert len(hw) == 131
    res = ok(h.parse_spo2_single(hw, h.ParseContext(header_time=ts)))
    assert [r.spo2 for r in res.records] == [99, 98]
    assert [r.timestamp for r in res.records] == [ts, ts + 600]
    legacy_ts = (T0 & 0xFFFFFF00) | 2
    legacy = w().u32(legacy_ts).u8(96).to_bytes()
    assert ok(h.parse_spo2_single(legacy, h.ParseContext(header_time=legacy_ts))).records[0].spo2 == 96


def test_spo2_sleep():
    assert ok(h.parse_spo2_sleep(w().u32(T0).u8(94).u8(10).to_bytes(), CTX)).records[0].time == 10
    v2 = rep(rep(rep(rep(w().u8(2).u32(T0).u8(93).u8(4), 6, 1), 6, 2), 8, 3), 4, 4).to_bytes()
    s = ok(h.parse_spo2_sleep(v2, dataclasses.replace(CTX, spo2_versioned=True))).records[0]
    assert s.spo2 == 93 and s.v2.signal_quality[0] == 3 and len(s.v2.extend) == 4


def test_spo2_osa_events():
    long = rep(rep(w().u32(T0).u8(5), 240, 90), 240, 60).to_bytes()
    e = ok(h.parse_spo2_osa_event(long, CTX)).records[0]
    assert e.spo2_decrease == 5 and len(e.heart_rate) == 240
    short = w().u8(1).u32(T0).u8(3).to_bytes()
    assert ok(h.parse_spo2_osa_event(short, dataclasses.replace(CTX, spo2_versioned=True))).records[0].spo2 is None


# ---- stress

def test_stress():
    single = w().u32(T0).u8(42).u32(T0 + 9).u8(0).to_bytes()
    s = ok(h.parse_stress_single(single, CTX))
    assert s.records[0].stress == 42 and s.records[1].is_likely_gap
    fail(h.parse_stress_single(single[:9], CTX))
    a = ok(h.parse_stress_all_day(bytes([10, 20, 0xFF]), CTX))
    assert [r.timestamp for r in a.records] == [T0, T0 + 60, T0 + 120]
    assert a.records[2].is_likely_gap and all(r.all_day for r in a.records)
    assert a.next_cursor == T0 + 181


# ---- skin temperature

def test_skin_temperature():
    p = (w().u16(2510).u16(3350).u32(0).u16(0x7FFF).u16(0x7FFF).u32(0).u16(2400).u16(1500).u32(0)
         .u16(-500).u16(3300).u32(0).to_bytes())
    res = ok(h.parse_skin_temperature(p, CTX))
    r = res.records
    assert (r[0].skin_centi_celsius, r[0].ambient_centi_celsius, r[0].skin_celsius) == (3350, 2510, 33.5)
    assert r[1].skin_centi_celsius is None and r[1].ambient_centi_celsius is None and r[1].skin_celsius is None
    assert r[2].skin_celsius is None  # 15.00 C is off-wrist
    assert r[3].ambient_centi_celsius == -500
    assert [x.timestamp for x in r] == [T0, T0 + 60, T0 + 120, T0 + 180]
    assert res.next_cursor == T0 + 241


# ---- HRV / resting family / respiration

def test_hrv_and_resting_family():
    assert ok(h.parse_hrv_rmssd(w().u32(T0).u8(4).u8(48).to_bytes(), CTX)).records[0] == h.HrvRmssd(T0, 4, 48)
    # Captured from the strap: ts 1792106880, tz +4 h, 60.56 ms.
    sdnn = ok(h.parse_hrv_sdnn(bytes.fromhex("8061d16a10a8170000"), CTX)).records[0]
    assert sdnn == h.HrvSdnn(1_792_106_880, 16, 6056) and abs(sdnn.sdnn_ms - 60.56) < 1e-9
    assert ok(h.parse_resting_hr(w().u32(T0).u8(0).u8(52).to_bytes(), CTX)).records[0] == \
        h.RestingHeartRate(T0, 0, 52)
    assert ok(h.parse_daily_max_hr(w().u32(T0).u8(0).u8(181).to_bytes(), CTX)).records[0] == \
        h.DailyMaxHeartRate(T0, 0, 181)
    assert ok(h.parse_hrv_rmssd(w().u32(T0).u8(-8).u8(30).to_bytes(), CTX)).records[0].tz_quarters == -8
    trunc = ok(h.parse_hrv_rmssd(w().u32(T0).u8(4).to_bytes(), CTX))
    assert trunc.records == [] and trunc.dropped_bytes == 5


def test_respiratory():
    r = ok(h.parse_sleep_breath_rate(w().u32(T0).u8(4).u8(14).u8(20).u8(1).to_bytes(), CTX)).records[0]
    assert r == h.SleepBreathRate(T0, 4, 14, 20, 1)


# ---- robustness and JSON

def test_all_parsers_survive_garbage():
    garbage = bytes((i * 37 + 11) & 0xFF for i in range(37))
    ctxs = [CTX, dataclasses.replace(CTX, spo2_versioned=True, activity_record_size=8)]
    for spec in h.SPECS:
        for n in range(len(garbage) + 1):
            for c in ctxs:
                r = spec.parser(garbage[:n], c)
                assert isinstance(r, (h.ParseSuccess, h.ParseFailure))


def test_to_json_includes_derived_fields():
    a = ok(h.parse_activity(bytes([0x71, 3, 9, 80]), CTX)).records[0]
    d = h.to_json(a)
    assert d["heart_rate"] == 80 and d["mode"] == 1 and d["is_worn"] and not d["is_blank"]
    s = ok(h.parse_spo2_sleep(rep(rep(rep(rep(w().u8(2).u32(T0).u8(93).u8(4), 6, 1), 6, 2), 8, 3), 4, 4).to_bytes(),
                              dataclasses.replace(CTX, spo2_versioned=True))).records[0]
    assert h.to_json(s)["v2"]["extend"] == "04040404"


# ---- types added in the second port

def test_today_extended():
    rec = w().u8(1).u8(0).u8(2).u8(10).u8(20).u8(30).u8(40).u8(0).u8(90).u8(5).u16(7000).to_bytes()
    res = ok(h.parse_today_activity_extended(rec + rec, CTX))
    assert len(res.records) == 2 and res.records[1].distance_cm == 7000 and res.records[0].steps == 90
    assert res.records[1].timestamp == T0 + 60 and res.next_cursor == T0 + 121
    fail(h.parse_today_activity_extended((rec + rec)[:23], CTX))


def test_spot_hr():
    p = w().u32(T0).u8(-8).u8(66).u32(T0 + 5).u8(4).u8(0).to_bytes()
    res = ok(h.parse_spot_hr(p, CTX))
    assert res.records[0].tz_quarters == -8 and res.records[0].heart_rate == 66 and res.records[1].heart_rate is None
    fail(h.parse_spot_hr(p[:7], CTX))


def test_osa_result_and_alt_events():
    osa = w().u32(T0).u32(3600).u32(12).u8(80).u8(1).u8(1).u8(2)
    for i in range(240):
        osa.u32(T0 + i).u32(-4).u32(30)
    o = ok(h.parse_osa_result(osa.to_bytes(), CTX)).records[0]
    assert o.odi_score == 80 and o.events[0].od_value == -4 and len(o.events) == 240
    assert ok(h.parse_spo2_osa_event_alt(w().u32(T0).u8(3).to_bytes(), CTX)).records[0].spo2_decrease == 3


def test_rest_hr_realtime_and_respiratory_event():
    r = ok(h.parse_rest_hr_realtime(w().u32(T0).u8(8).u8(55).u8(57).u8(2).to_bytes(), CTX)).records[0]
    assert r == h.RestHrRealtime(T0, 8, 55, 57, 2)
    e = ok(h.parse_sleep_respiratory_event(rep(w().u32(T0).u8(4), 564, 7).to_bytes(), CTX)).records[0]
    assert e.timestamp == T0 and len(e.raw) == 569


def pai_payload(version: int) -> bytes:
    b = w().u8(version).u32(T0).u32(8)
    f = lambda x: b.bytes(struct.pack("<f", x))  # noqa: E731
    for i in range(7):
        f(i + 0.5)
    f(1.5); f(2.5); f(3.5)  # noqa: E702
    b.u16(10).u16(20).u16(30)
    f(7.5); f(99.25)  # noqa: E702
    b.u16(55).u16(190).u8(3).u8(1)
    if version >= 2:
        for i in range(7):
            f(10.0 + i)
    if version >= 4:
        b.u8(1).u8(35)
    if version >= 5:
        b.u8(100).u8(130).u8(160)
    return b.to_bytes()


def test_pai_versions():
    v1 = ok(h.parse_pai(pai_payload(1), CTX)).records[0]
    assert v1.timestamp == T0 and v1.total_pai == 99.25
    assert v1.activity_scores == [0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5]
    assert v1.max_hr == 190 and v1.next_activity_scores is None and v1.gender is None
    v5 = ok(h.parse_pai(pai_payload(5), CTX))
    r5 = v5.records[0]
    assert r5.next_activity_scores[6] == 16.0 and r5.age == 35 and r5.high_zone_limit == 160
    assert v5.next_cursor == T0 + 1
    fail(h.parse_pai(pai_payload(5)[:80], CTX))
    fail(h.parse_pai(pai_payload(1)[:30], CTX))


def test_device_statistic_and_raw():
    assert ok(h.parse_device_statistic(b"\x01\x02", CTX)).records[0] == h.RawHistoryRecord(44, T0, b"\x01\x02")
    assert ok(h.parse_raw(85)(b"\xaa", CTX)).records[0].code == 85
    assert ok(h.parse_raw(85)(b"", CTX)).records == []


def test_workout_parsers_survive_garbage():
    from zebrastrap import workouts as wk
    garbage = bytes((i * 37 + 11) & 0xFF for i in range(37))
    for p in (wk.parse_summary, wk.parse_detail, wk.parse_sport_effect, wk.parse_recovery_hr, h.parse_pai):
        for n in range(len(garbage) + 1):
            assert isinstance(p(garbage[:n], CTX), (h.ParseSuccess, h.ParseFailure))
