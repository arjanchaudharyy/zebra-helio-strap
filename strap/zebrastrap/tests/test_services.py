"""ZeppChannelJobsTest.kt and ReadinessCodecTest (AlarmsAndSettingsTest.kt)."""

import struct

from zebrastrap import history as h
from zebrastrap.services import PaiCodec, ReadinessCodec, SleepModuleCodec

hx = bytes.fromhex


def test_versions():
    # Probed on firmware 3.27.0.1.
    assert ReadinessCodec.parse_version(hx("0100033c3c01fe")) == 3
    assert PaiCodec.parse_version(hx("010005")) == 5
    assert SleepModuleCodec.parse_version(hx("020301312e302e3200342e302e313700")) == 3
    assert PaiCodec.parse_version(hx("0d00")) is None


def test_requests():
    assert PaiCodec.query(100_000_000).hex() == "0c00e1f505"
    assert ReadinessCodec.request(ReadinessCodec.DATA, 100_000_000).hex() == "0600e1f505"
    assert ReadinessCodec.request(ReadinessCodec.STRESS_THRESHOLD, 100_000_000).hex() == "0c00e1f505"
    assert ReadinessCodec.score_request(0x6AAAD946) == hx("0846d9aa6a")


def test_readiness_records_kept_whole():
    reply = ("0d0002" + "00e1f505" + "10" + "32" + "01e1f505" + "00000000"
             + "80e1f505" + "10" + "33" + "81e1f505" + "00000000")
    records = ReadinessCodec.parse_records(ReadinessCodec.STRESS_THRESHOLD, hx(reply))
    assert [r.timestamp for r in records] == [100_000_000, 100_000_128]
    assert len(records[0].raw) == 14
    assert ReadinessCodec.parse_records(ReadinessCodec.DATA, hx("070200")) == []
    assert ReadinessCodec.parse_records(ReadinessCodec.DATA, hx("070500")) is None
    assert ReadinessCodec.parse_records(ReadinessCodec.DATA, hx("0d0000")) is None


def pai_record(t: int, daily: float, version: int = 5) -> bytes:
    b = struct.pack("<BII", version, t, 16) + struct.pack("<10f", *([0.0] * 10)) + struct.pack("<3H", 0, 0, 0)
    b += struct.pack("<ffHHBB", daily, 80.0, 52, 180, 0, 0)
    return b + bytes(102 - len(b))


def test_pai_records_are_102_bytes():
    reply = bytes([0x0D, 0, 2]) + pai_record(1_000, 12.5, version=3) + pai_record(87_400, 3.0)
    records = PaiCodec.parse(reply)
    assert [r.timestamp for r in records] == [1_000, 87_400]
    assert [r.daily_pai for r in records] == [12.5, 3.0]
    assert [r.version for r in records] == [3, 5]
    assert records[0].rest_hr == 52
    assert PaiCodec.parse(bytes([0x0D, 1])) == []
    assert PaiCodec.parse(bytes([0x09, 0, 0])) is None
    assert PaiCodec.parse(bytes([0x0D, 0, 0])) == []


def test_sleep_plan_structs():
    struct59 = (1_000).to_bytes(4, "little") + bytes(55)
    two = struct59 + (2_000).to_bytes(4, "little") + bytes(55)
    r = h.parse_sleep_plan(two, h.ParseContext(500))
    assert [x.timestamp for x in r.records] == [1_000, 2_000] and r.next_cursor == 2_001
    assert len(r.records[0].raw) == 59
    assert len(h.parse_sleep_plan(bytes(64), h.ParseContext(500)).records[0].raw) == 64


SCORE_RECORD = hx("c030d16aa893d16a10043e420104035bffff59645664ffffffffff7fff000080bf0000001fffffffffffffff")


def test_readiness_score_from_strap():
    r = ReadinessCodec.parse_scores(hx("090001") + SCORE_RECORD)[0]
    assert r.day == 1_792_094_400 and r.score == 91
    assert (r.hrv_score, r.resting_hr_score, r.skin_temperature_score, r.breathing_score) == (89, 86, 100, 100)
    assert r.physical_score is None and r.mental_score is None
    assert (r.sleep_hrv, r.sleep_resting_hr) == (62, 66)
    assert r.algorithm_version == "4.3"


def test_readiness_records_are_44_bytes_apart():
    assert len(SCORE_RECORD) == 44
    second = bytearray(SCORE_RECORD)
    second[0] = 0x40
    second[15] = 77
    scores = ReadinessCodec.parse_scores(hx("090002") + SCORE_RECORD + bytes(second))
    assert [s.score for s in scores] == [91, 77]
    assert [s.day for s in scores] == [1_792_094_400, 1_792_094_272]


def test_readiness_no_data():
    assert ReadinessCodec.parse_scores(hx("0902") + b"\x00") == []
    assert ReadinessCodec.parse_scores(hx("0d0000")) is None
