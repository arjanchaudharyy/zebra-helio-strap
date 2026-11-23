"""WorkoutParsersTest.kt, the workout cases of GdspClientTest.kt, and recoveryHr from RecordParsersTest.kt.

Fixtures are the Kotlin test resources: one 2026-10-16 workout captured from a Helio Strap (fw 3.25.0.3).
"""

import asyncio
from datetime import timezone
from pathlib import Path

import pytest

from zebrastrap import workouts as wk
from zebrastrap._bytes import ByteWriter
from zebrastrap.history import ParseContext, ParseFailure, ParseSuccess
from zebrastrap.tests.test_history import SimulatedGdspStrap, _client

TRACK = 1_792_141_453
FIX = Path(__file__).parent / "fixtures"


def fixture(name: str) -> bytes:
    return (FIX / name).read_bytes()


def test_summary():
    r = wk.parse_summary(fixture("summary_5.bin"), ParseContext(TRACK))
    assert isinstance(r, ParseSuccess)
    w = r.records[0]
    assert w.track_id == TRACK and w.sport_type == 223
    assert (w.total_time, w.exercise_time, w.calories) == (530, 530, 66)
    assert (w.average_heart_rate, w.max_heart_rate, w.min_heart_rate) == (119, 132, 96)
    assert w.aerobic_training_effect == pytest.approx(1.0, abs=1e-6)
    assert w.anaerobic_training_effect == pytest.approx(0.1, abs=1e-6)
    assert w.training_load == 6 and w.vo2max is None
    assert r.next_cursor == TRACK + 1


def test_summary_rejects_legacy_layouts_and_garbage():
    assert isinstance(wk.parse_summary(bytes([2, 1, 0, 0]), ParseContext(TRACK)), ParseFailure)
    assert wk.parse_summary(b"\x00\x80", ParseContext(TRACK)).records == []
    assert isinstance(wk.parse_summary(b"\x00\x80\x1a\x05\x08", ParseContext(TRACK)), ParseFailure)


def test_detail_heart_rate():
    r = wk.parse_detail(fixture("detail_6.bin"), ParseContext(TRACK))
    d = r.records[0]
    assert len(d.heart_rate) == 1307
    assert d.heart_rate[0] == (1_792_141_453_710, 96)
    assert all(400 <= b[0] - a[0] <= 4000 for a, b in zip(d.heart_rate, d.heart_rate[1:]))
    assert d.heart_rate[-1][0] // 1000 == 1_792_142_763
    assert [e[1] for e in d.events] == [1, 6]
    assert r.next_cursor == TRACK + 1


def test_detail_event_code_is_u16():
    base = 1_792_141_453_000
    ts = bytes(4) + base.to_bytes(8, "little")
    payload = bytes([0x01, 12]) + ts + bytes.fromhex("04040a000201")
    d = wk.parse_detail(payload, ParseContext(TRACK)).records[0]
    assert d.events == [(base + 10, 0x0102)]


def test_detail_overrun_fails():
    assert isinstance(wk.parse_detail(bytes([0x08, 10, 1]), ParseContext(TRACK)), ParseFailure)


def test_sport_effect():
    r = wk.parse_sport_effect(fixture("effect_59.bin"), ParseContext(1_792_142_584))
    e = r.records[0]
    assert e.timestamp == 1_792_142_584 and e.maximal_heart_rate == 191
    assert e.recovery_time_minutes == 127 and e.vo2max is None
    assert e.training_load_weekly == pytest.approx(6.826, abs=1e-3)
    assert e.training_load_monthly == pytest.approx(27.3, abs=0.01)
    assert (e.optimal_load_min, e.optimal_load_max) == (54, 182)


def test_recovery_hr():
    t0 = 1_760_000_000
    p = ByteWriter().u32(t0).u32(3).u8(150).u8(140).u8(130).u32(t0 + 5000).u32(10).u8(1).to_bytes()
    res = wk.parse_recovery_hr(p, ParseContext(t0))
    assert len(res.records) == 1  # second block truncated -> stop
    assert res.records[0].timestamp == t0 and res.records[0].samples == [150, 140, 130]
    assert res.next_cursor == t0 + 1


def test_proto_reader():
    # field 1 varint 300, field 2 fixed32 1.5f, field 3 message {1: 7}, field 4 string "ab"
    b = bytes.fromhex("08ac02" "150000c03f" "1a020807" "22026162")
    m = wk.Proto(b)
    assert m.ints[1] == 300 and m.floats[2] == 1.5 and m.messages[3].int(1) == 7
    with pytest.raises(ValueError):
        wk.Proto(bytes.fromhex("08ff"))  # truncated varint
    with pytest.raises(ValueError):
        wk.Proto(bytes.fromhex("0b"))  # wire type 3
    assert wk.Proto(bytes.fromhex("08ffffffff0f")).int(1) == -1


def _strap(workouts: dict):
    return SimulatedGdspStrap({5: [(t, s, False) for t, (s, d) in workouts.items()],
                               6: [(t, d, False) for t, (s, d) in workouts.items()]})


def run(coro):
    return asyncio.run(asyncio.wait_for(coro, 20))


def test_detail_follows_summary_with_the_same_cursor():
    async def go():
        strap = _strap({TRACK: (fixture("summary_5.bin"), fixture("detail_6.bin"))})
        c, _ = _client(strap)
        since = TRACK - 3600
        w = await wk.fetch_workout(c, since, timezone.utc)
        assert isinstance(w, wk.Workout)
        assert w.summary.track_id == TRACK and w.summary.sport_type == 223
        assert w.detail.timestamp == TRACK and len(w.detail.heart_rate) == 1307
        assert w.next_cursor == TRACK + 1
        assert strap.starts == [(5, since), (6, since)] and strap.acks == [1, 1]
        assert isinstance(await wk.fetch_workout(c, w.next_cursor, timezone.utc), wk.WorkoutNone)
        assert strap.starts[-1] == (5, TRACK + 1) and len(strap.acks) == 3
    run(go())


def test_an_unreadable_summary_is_left_out_and_the_next_one_still_comes():
    async def go():
        odd = TRACK - 3600
        strap = _strap({odd: (bytes([1, 0, 0, 0]), fixture("detail_6.bin")),
                        TRACK: (fixture("summary_5.bin"), fixture("detail_6.bin"))})
        c, _ = _client(strap)
        u = await wk.fetch_workout(c, odd - 60, timezone.utc)
        assert isinstance(u, wk.WorkoutUnreadable) and u.next_cursor == odd + 1
        assert strap.starts == [(5, odd - 60), (6, odd - 60)]
        w = await wk.fetch_workout(c, u.next_cursor, timezone.utc)
        assert w.summary.track_id == TRACK
        res = await wk.sync_workouts(c, odd - 60, timezone.utc, now=TRACK + 10_000)
        assert [x.summary.track_id for x in res.workouts] == [TRACK] and not res.ok  # one unreadable
        assert res.saved_cursor == TRACK + 1
    run(go())


def test_fast_recharge_is_acknowledged_with_33():
    async def go():
        strap = _strap({})
        strap.fast_recharge = True
        c, _ = _client(strap)
        w = await wk.fetch_workout(c, TRACK, timezone.utc)
        assert isinstance(w, wk.WorkoutFailed) and w.fast_recharge
        assert strap.acks == [0x33]
    run(go())
