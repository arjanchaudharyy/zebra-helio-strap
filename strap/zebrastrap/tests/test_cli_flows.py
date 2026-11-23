"""End-to-end runs of the authenticated commands against a simulated strap (no radio)."""

import argparse
import asyncio
import json
import time


from zebrastrap import auth, cli, controls, transport
from zebrastrap.auth import Endpoints
from zebrastrap.session import ChannelSession
from zebrastrap.tests.test_auth import AUTH_KEY, SimulatedStrap
from zebrastrap.tests.test_controls import HEALTH
from zebrastrap.tests.test_history import SimulatedGdspStrap
from zebrastrap.tests.test_parsers import sleep_record
from zebrastrap.tests.test_services import SCORE_RECORD, pai_record
from zebrastrap.tests.test_workouts import fixture
from zebrastrap._bytes import ByteWriter

SERVICES = bytes.fromhex(("04 0a00 0000 00 8200 00 1500 00 4b00 00 1800 01 1a00 01 0a00 01"
                          " 4900 00 2200 00 3200 00").replace(" ", ""))


class FullStrap(SimulatedStrap):
    """SimulatedStrap plus GDSP on 0x004B (data on 0005) and the encrypted control endpoints."""

    def __init__(self, key, gdsp: SimulatedGdspStrap):
        super().__init__(key)
        self.gdsp = gdsp
        self.on_history_data = None
        self.settings_writes = []
        self.pai_time = 1_792_000_000
        gdsp.client = self  # the GDSP sim replies through on_control/on_data below

    # GDSP sim -> phone
    def on_control(self, b: bytes):
        self.outbox.append((Endpoints.HISTORY, b))
        self.flush()

    def on_data(self, p: bytes):
        self.on_history_data(p)

    def respond(self, endpoint, payload):
        if endpoint == Endpoints.SERVICES:
            self.outbox.append((endpoint, SERVICES))
        elif endpoint == Endpoints.HISTORY:
            asyncio.ensure_future(self.gdsp.send(payload))
        elif endpoint == Endpoints.READINESS:
            replies = {0x00: "0100033c3c01fe", 0x08: "090001" + SCORE_RECORD.hex(),
                       0x0C: "0d0001" + "00e1f505" + "10" + "32" + "01e1f505" + "00000000", 0x06: "070200"}
            self.outbox.append((endpoint, bytes.fromhex(replies[payload[0]])))
        elif endpoint == Endpoints.PAI:
            if payload[0] == 0:
                self.outbox.append((endpoint, bytes.fromhex("010005")))
            else:
                self.outbox.append((endpoint, bytes([0x0D, 0, 1]) + pai_record(self.pai_time, 12.5)))
        elif endpoint == Endpoints.SLEEP:
            self.outbox.append((endpoint, bytes.fromhex("0203")))
        elif endpoint == Endpoints.MOTOR_VIBRATE:
            self.outbox.append((endpoint, b"\x04\x01"))
        elif endpoint == Endpoints.FIND_DEVICE:
            if payload == controls.FindDevice.BUZZ:
                self.outbox.append((endpoint, b"\x04\x01\x01"))
        elif endpoint == Endpoints.SETTINGS:
            if payload == b"\x01":
                self.outbox.append((endpoint, b"\x02\x03"))
            elif payload[0] == 3:
                self.outbox.append((endpoint, HEALTH))
            elif payload[0] == 5:
                self.settings_writes.append(payload)
                self.outbox.append((endpoint, b"\x06\x01"))
        else:
            super().respond(endpoint, payload)


class FakeClient:
    async def disconnect(self):
        pass


def _patch(monkeypatch, strap: FullStrap):
    async def fake_authed(args, on_history_data=None):
        session = ChannelSession(strap)
        strap.session = session
        strap.on_history_data = on_history_data
        session.start()
        await auth.authenticate(session, args.key_bytes)
        return FakeClient(), session

    monkeypatch.setattr(cli, "_authed", fake_authed)


def _args(tmp_path, **kw):
    base = dict(command="sync", key=None, key_bytes=AUTH_KEY, address=None, seconds=1, days=1.0, types=None,
                out=str(tmp_path), pattern="200,150,200", verbose=False)
    base.update(kw)
    return argparse.Namespace(**base)


def test_sync_writes_json_lines_and_cursors(monkeypatch, tmp_path):
    now = int(time.time()) // 60 * 60
    header = now - 120
    monkeypatch.setattr(time, "time", lambda: now + 5)  # no race with the minute boundary
    # two written minutes, then three unwritten slots (dated now, now+60, now+120)
    act = bytes.fromhex("702300500d00000070430a5005000000" + "ff" * 24)
    hrv = bytes.fromhex("8061d16a10a8170000")
    gdsp = SimulatedGdspStrap({1: [(header, act, False)], 78: [(header, hrv, False)]})
    gdsp.size_as_records = None
    strap = FullStrap(AUTH_KEY, gdsp)
    _patch(monkeypatch, strap)

    # the GDSP version reply: types 1 and 78 offered
    orig_send = gdsp.send

    async def send(b):
        if b == b"\x00":
            return gdsp.reply(bytes.fromhex("100001 03 00 0200 0101 4e01".replace(" ", "")))
        await orig_send(b)
    gdsp.send = send

    rc = asyncio.run(asyncio.wait_for(cli.cmd_sync(_args(tmp_path)), 60))
    assert rc == 0
    lines = [json.loads(x) for x in (tmp_path / "01_activity.jsonl").read_text().splitlines()]
    assert [x["timestamp"] for x in lines] == [header, header + 60]
    assert lines[1]["heart_rate"] == 80 and lines[0]["type"] == 1
    sdnn = json.loads((tmp_path / "78_hrv_sdnn.jsonl").read_text())
    assert sdnn["sdnn_centi_ms"] == 6056
    cursors = json.loads((tmp_path / "cursors.json").read_text())
    # activity keeps the blank minute at `now` but not the future ones: cursor = now + 60
    assert cursors["1"] == now + 60
    assert cursors["78"] == 1_792_106_881
    assert gdsp.acks.count(1) == len(gdsp.acks)


def test_vibrate_find_and_hr_interval(monkeypatch, tmp_path):
    strap = FullStrap(AUTH_KEY, SimulatedGdspStrap({}))
    _patch(monkeypatch, strap)
    assert asyncio.run(asyncio.wait_for(cli.cmd_vibrate(_args(tmp_path, command="vibrate")), 30)) == 0
    vib = [r for r in strap.received if r[0] == Endpoints.MOTOR_VIBRATE]
    assert vib[0][1] == controls.MotorVibrate.play([200, 150, 200])
    assert vib[0][2] & transport.ENCRYPTED  # 0018 requires encryption

    async def go():
        session = ChannelSession(strap)
        strap.session = session
        strap.cipher = None
        session.start()
        await auth.authenticate(session, AUTH_KEY)
        assert await controls.find_device_start(session)
        await controls.find_device_stop(session)
        assert await controls.set_hr_interval(session, 10)
        await session.stop()
    asyncio.run(asyncio.wait_for(go(), 30))
    finds = [r[1] for r in strap.received if r[0] == Endpoints.FIND_DEVICE]
    assert finds == [b"\x03", b"\x06"]
    assert strap.settings_writes == [bytes.fromhex("050803000101100a")]


def _version_reply(types) -> bytes:
    w = ByteWriter().u8(0x10).u8(0).u8(1).u8(3).u8(0).u16(len(types))
    for t in types:
        w.u8(t).u8(1)
    return w.to_bytes()


def test_full_sync_follows_zepp_order_and_dedupes_on_resync(monkeypatch, tmp_path):
    now = int(time.time()) // 60 * 60
    hdr = now - 3600
    monkeypatch.setattr(time, "time", lambda: now + 5)
    track = hdr + 7
    gdsp = SimulatedGdspStrap({
        1: [(hdr, bytes.fromhex("702300500d00000070430a5005000000"), False)],
        5: [(hdr, fixture("summary_5.bin"), False)],
        6: [(hdr, fixture("detail_6.bin"), False)],
        72: [(hdr, sleep_record(hdr, [(0, 120, 4)]), False)],
        58: [(hdr, ByteWriter().u32(hdr).u8(0).u8(52).to_bytes(), False)],
        37: [(hdr, ByteWriter().u32(hdr).u8(97).to_bytes(), False)],
        87: [(hdr, ByteWriter().u32(hdr).u16(2).u8(60).u8(61).to_bytes(), True)],
        85: [(hdr, bytes.fromhex("deadbeef"), False)],
    })
    advertised = [1, 5, 6, 37, 58, 72, 85, 87]
    orig = gdsp.send

    async def send(b):
        if b == b"\x00":
            return gdsp.reply(_version_reply(advertised))
        await orig(b)
    gdsp.send = send
    strap = FullStrap(AUTH_KEY, gdsp)
    _patch(monkeypatch, strap)

    def line_counts():
        return {p.name: len(p.read_text().splitlines()) for p in tmp_path.glob("*.jsonl")}

    assert asyncio.run(asyncio.wait_for(cli.cmd_sync(_args(tmp_path)), 60)) == 0
    first = line_counts()
    assert first == {"58_resting_heart_rate.jsonl": 1, "72_sleep_result.jsonl": 1, "01_activity.jsonl": 2,
                     "readiness_08.jsonl": 1, "05_workout_summary.jsonl": 1, "06_workout_detail.jsonl": 1,
                     "readiness_0c.jsonl": 1, "37_spo2_single.jsonl": 1, "13_pai.jsonl": 1,
                     "87_sec_hr_v2.jsonl": 2, "85_raw.jsonl": 1}
    # START order follows Zepp's list, then the raw type
    order = []
    for typ, _ in gdsp.starts:
        if not order or order[-1] != typ:
            order.append(typ)
    # 5 -> 6, then a second START(5) after the workout that finds nothing; PAI goes to 0x0022, not GDSP 13
    assert order == [58, 66, 72, 1, 5, 6, 5, 37, 56, 57, 87, 85]
    summary = json.loads((tmp_path / "05_workout_summary.jsonl").read_text())
    assert summary["sport_type"] == 223
    raw = json.loads((tmp_path / "85_raw.jsonl").read_text())
    assert raw["raw"] == "deadbeef" and raw["code"] == 85
    sec = [json.loads(x) for x in (tmp_path / "87_sec_hr_v2.jsonl").read_text().splitlines()]
    assert sec[0]["heart_rate"] == 60
    # a second sync from the same start writes no duplicate lines
    strap2 = FullStrap(AUTH_KEY, gdsp)
    gdsp.client = strap2
    _patch(monkeypatch, strap2)
    assert asyncio.run(asyncio.wait_for(cli.cmd_sync(_args(tmp_path)), 60)) == 0
    assert line_counts() == first


def test_sync_types_flag(monkeypatch, tmp_path):
    gdsp = SimulatedGdspStrap({})
    orig = gdsp.send

    async def send(b):
        if b == b"\x00":
            return gdsp.reply(_version_reply([1, 72, 85]))
        await orig(b)
    gdsp.send = send
    strap = FullStrap(AUTH_KEY, gdsp)
    _patch(monkeypatch, strap)
    assert asyncio.run(asyncio.wait_for(cli.cmd_sync(_args(tmp_path, types="72,pai")), 60)) == 0
    assert {t for t, _ in gdsp.starts} == {72}
    assert any(r[0] == Endpoints.PAI and r[1][0] == 0x0C for r in strap.received)
