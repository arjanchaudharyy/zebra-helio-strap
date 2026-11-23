"""FetchCodecTest.kt, VersionReplyTest, ActivityLayoutTest, ActivityPaddingTest and GdspClientTest.kt.

The GdspClient tests use activity and type-87 payloads, because the workout parsers (types 5/6) are not ported.
"""

import asyncio
import zlib
from datetime import datetime, timezone
from zoneinfo import ZoneInfo

from zebrastrap import history as h
from zebrastrap._bytes import ByteReader, ByteWriter
from zebrastrap.crypto import crc32

# ---- codec -------------------------------------------------------------------------------------------


def test_simple_commands():
    assert h.query_version().hex() == "00"
    assert h.transfer().hex() == "02"
    assert h.ack().hex() == "03"
    assert h.ack(1).hex() == "0301"
    assert h.ack(h.CYCLIC_SYNC).hex() == "03fa"
    assert h.query_data_status().hex() == "05"


def test_start_fetch_with_dst_offset():
    t = datetime(2026, 7, 15, 13, 45, 30, tzinfo=ZoneInfo("Europe/Berlin"))
    assert h.start_fetch(1, t).hex() == "01" "01" "ea07" "07" "0f" "0d" "2d" "1e" "08" "01"


def test_start_fetch_winter_negative_and_half_hour_offsets():
    winter = h.start_fetch(72, datetime(2026, 1, 2, tzinfo=ZoneInfo("Europe/Berlin")), flag=None)
    assert len(winter) == 10 and winter[9] == 4 and winter[1] == 72
    ny = h.start_fetch(13, datetime(2026, 1, 2, tzinfo=ZoneInfo("America/New_York")))
    assert ny[9] - 256 == -20
    india = h.start_fetch(13, datetime(2026, 1, 2, tzinfo=ZoneInfo("Asia/Kolkata")))
    assert india[9] == 22
    # -3:30 truncates toward zero: -14 quarters
    st_johns = h.GdspTime.of_datetime(datetime(2026, 1, 2, tzinfo=ZoneInfo("America/St_Johns")))
    assert st_johns.tz_quarters == -14


def test_start_fetch_from_epoch_matches_zoned():
    zone = ZoneInfo("Europe/Berlin")
    zdt = datetime(2026, 3, 29, 3, 30, tzinfo=zone)  # right after the DST switch
    assert h.start_fetch(1, zdt) == h.start_fetch_epoch(1, int(zdt.timestamp()), zone)


def test_gdsp_time_round_trip():
    zdt = datetime(2026, 10, 16, 23, 59, 1, tzinfo=ZoneInfo("America/Los_Angeles"))
    t = h.GdspTime.of_datetime(zdt)
    assert t.tz_quarters == -28
    assert t.to_epoch_second() == int(zdt.timestamp())
    w = ByteWriter()
    t.write_to(w)
    assert h.GdspTime.read(ByteReader(w.to_bytes())) == t


def test_parse_start_reply():
    b = (ByteWriter().u8(0x10).u8(0x01).u8(0x01).u32(1234)
         .u16(2026).u8(10).u8(16).u8(8).u8(0).u8(0).u8(8).u8(0x01).to_bytes())
    m = h.parse(b)
    assert isinstance(m, h.StartReply) and m.is_success and m.size == 1234
    assert m.first_record_epoch == int(datetime(2026, 10, 16, 8, tzinfo=ZoneInfo("Europe/Berlin")).timestamp())
    assert m.expansion == b"\x01" and m.is_compressed is True


def test_parse_start_reply_empty_and_errors():
    e = h.parse(bytes.fromhex("100101 00000000 ea070a1008000008".replace(" ", "")))
    assert e.size == 0 and e.is_compressed is None
    busy = h.parse(bytes.fromhex("100107"))
    assert isinstance(busy, h.StartReply) and not busy.is_success
    assert busy.status == h.GdspStatus.BUSY and busy.first_record_time is None
    for raw in ("10010100", "10", "", "99"):
        assert isinstance(h.parse(bytes.fromhex(raw)), h.Malformed)


def test_parse_transfer_complete_ack_and_version():
    done = h.parse(bytes.fromhex("100201"))
    assert isinstance(done, h.TransferComplete) and done.is_success and done.crc32 is None
    assert h.parse(bytes.fromhex("10020178563412")).crc32 == 0x12345678
    assert isinstance(h.parse(bytes.fromhex("100301")), h.AckReply)
    ver = h.parse(bytes.fromhex("100001030002000101 4801".replace(" ", "")))
    assert ver.version == 3 and ver.supported_types() == {1, 0x48}
    other = h.parse(bytes.fromhex("10050e"))
    assert isinstance(other, h.OtherReply) and other.status == h.GdspStatus.DATA_SIZE_MISMATCH
    assert h.GdspStatus.from_code(200) == h.GdspStatus.UNKNOWN


def test_parse_data_available():
    assert h.parse(bytes.fromhex("1105")).types_to_sync == [5, 6, 79]
    assert h.parse(bytes.fromhex("1106")).types_to_sync == [5, 6, 79]
    assert h.parse(bytes.fromhex("1146")).types_to_sync == [70]
    assert h.parse(bytes.fromhex("1160")).types_to_sync == [96]
    assert h.parse(bytes.fromhex("1101")).types_to_sync == []
    assert isinstance(h.parse(bytes.fromhex("11")), h.Malformed)


def test_version_reply_captured_from_strap():
    assert h.parse(bytes.fromhex("1000010300030007010101 4801".replace(" ", ""))).supported_types() == {7, 1, 0x48}
    assert h.parse(bytes.fromhex("100001030003000701010148 0101".replace(" ", ""))).supported_types() == {7, 1, 0x48}
    assert h.parse(bytes.fromhex("1000010300030007010101")).supported_types() is None


def test_inflate_and_is_zlib():
    raw = bytes(range(200)) * 3
    z = zlib.compress(raw)
    assert h.is_zlib(z) and h.inflate(z) == raw
    try:
        h.inflate(z[:-5])
        raise AssertionError("no error")
    except Exception:
        pass


def test_plan_and_loop_rule():
    assert {s.code for s in h.plan({1, 72, 3})} == {1, 72}
    assert all(s.strap == h.EXPECTED for s in h.plan(None))
    assert 87 not in {s.code for s in h.plan(None)}
    assert 87 in {s.code for s in h.plan({87})}
    assert h.SPECS[0].code == 1
    act = h.SPECS_BY_CODE[1]
    assert h.should_continue(act, 100, 101)
    assert not h.should_continue(act, 100, 100)
    assert not h.should_continue(act, 100, None)
    assert not h.should_continue(h.SPECS_BY_CODE[18], 100, 200)


# ---- activity layout ------------------------------------------------------------------------------


def w8(s: str) -> bytes:
    return bytes.fromhex(s.replace(" ", ""))


def test_detects_record_size():
    assert h.detect_activity_record_size(bytes([1] * 12)) == 4
    four = bytes({0: 1, 4: 0x3A}.get(i % 8, 40) for i in range(64))
    assert h.detect_activity_record_size(four) == 8
    vals = [5, 3, 0, 55, 200, 180, 90, 140]
    assert h.detect_activity_record_size(bytes(vals[i % 8] for i in range(64))) == 8
    strap = w8("7603 00ff 0500 0000 7047 0051 0500 0000 7038 0857 0500 0000 ffff ffff ffff ffff")
    assert h.detect_activity_record_size(strap) == 8


def test_record_size_from_size_field():
    payload = bytes([1] * 24)
    assert h.activity_record_size(payload, 3, 4) == 8
    assert h.activity_record_size(payload, 0, 4) == 4
    assert h.activity_record_size(payload, 0, 8) == 8
    assert h.activity_record_size(payload, 0) == h.detect_activity_record_size(payload)


def _act(hexs: str):
    return h.parse_activity(bytes.fromhex(hexs), h.ParseContext(1000, activity_record_size=8))


def test_trailing_ff_records_are_minutes_too():
    r = _act("702300500d00000070430a5005000000ffffffffffffffffffffffffffffffff")
    assert [x.timestamp for x in r.records] == [1000, 1060, 1120, 1180]
    assert [x.is_blank for x in r.records] == [False, False, True, True]
    assert r.next_cursor == 1240


def test_written_minutes_after_unwritten_ones_keep_their_times():
    r = _act("603300500500000060431a5207000000" + "ff" * 16 + "7300000005000000" * 3)
    assert len(r.records) == 7 and r.records[4].timestamp == 1240 and not r.records[4].is_blank
    assert r.next_cursor == 1420


def test_leading_ff_and_all_ff():
    r = _act("ff" * 8 + "5016004705000000" + "ff" * 8 + "500e004605000000")
    assert [x.is_blank for x in r.records] == [True, False, True, False] and r.next_cursor == 1240
    r = _act("ff" * 24)
    assert all(x.is_blank for x in r.records) and r.next_cursor == 1180


# ---- GDSP client against a simulated strap ---------------------------------------------------------


class SimulatedGdspStrap:
    """Device side of GDSP: START replies, indexed 0005 packets, `10 02 st crc32`, and ACKs.

    chunks maps type -> list of (header_epoch, payload, compressed).
    """

    def __init__(self, chunks):
        self.chunks = chunks
        self.client: h.GdspClient = None  # type: ignore[assignment]
        self.starts = []
        self.acks = []
        self.fast_recharge = False
        self.silent_starts = 0
        self.invalid_state_starts = 0
        self.next_status = None
        self.drop_packet = None
        self.double_packets = False
        self.bad_crc = False
        self.header = None
        self.push_before_start = None
        self.pending = None
        self.size_as_records = None

    def reply(self, b: bytes):
        self.client.on_control(b)

    async def send(self, b: bytes):
        op = b[0]
        if op == h.OP_START:
            typ = b[1]
            since = h.GdspTime.read(ByteReader(b, 2)).to_epoch_second()
            self.starts.append((typ, since))
            if self.push_before_start is not None:
                self.reply(bytes([0x11, self.push_before_start]))
                self.push_before_start = None
            if self.fast_recharge:
                return self.reply(bytes([0x10, 1, h.GdspStatus.FAST_RECHARGE_MODE]))
            if self.silent_starts:
                self.silent_starts -= 1
                return
            if self.invalid_state_starts:
                self.invalid_state_starts -= 1
                return self.reply(bytes([0x10, 1, h.GdspStatus.INVALID_STATE]))
            if self.next_status is not None:
                st, self.next_status = self.next_status, None
                return self.reply(bytes([0x10, 1, st]))
            found = next(((t, p, c) for t, p, c in sorted(self.chunks.get(typ, [])) if t >= since), None)
            self.pending = found
            raw = (zlib.compress(found[1]) if found[2] else found[1]) if found else b""
            self.pending_raw = raw
            size = len(found[1]) if found else 0
            if found and self.size_as_records:
                size //= self.size_as_records
            w = ByteWriter().u8(0x10).u8(1).u8(1).u32(size)
            h.GdspTime.of(self.header or (found[0] if found else since), timezone.utc).write_to(w)
            w.u8(1 if found and found[2] else 0)
            self.reply(w.to_bytes())
        elif op == h.OP_TRANSFER:
            raw = self.pending_raw if self.pending else b""
            for i in range(0, max(len(raw), 1), 19):
                idx = i // 19
                if idx == self.drop_packet or not raw:
                    continue
                pkt = bytes([idx & 0xFF]) + raw[i:i + 19]
                self.client.on_data(pkt)
                if self.double_packets:
                    self.client.on_data(pkt)
            self.drop_packet, self.double_packets = None, False
            self.reply(ByteWriter().u8(0x10).u8(2).u8(1).u32(crc32(raw) ^ (1 if self.bad_crc else 0)).to_bytes())
            self.bad_crc = False
        elif op == h.OP_ACK:
            self.acks.append(b[1] if len(b) > 1 else None)
            self.pending = None
            self.reply(bytes([0x10, 3, 1]))


T0 = 1_792_141_440  # a whole minute
ACT = bytes.fromhex("702300500d00000070430a5005000000" * 4)  # 8 minutes, 64 bytes


def _client(strap, **kw):
    logs = []
    c = h.GdspClient(strap.send, log=logs.append, timeout=0.3, trailing=0.02, **kw)
    strap.client = c
    return c, logs


def run(coro):
    return asyncio.run(asyncio.wait_for(coro, 20))


def test_chunk_fetch_and_ack():
    async def go():
        strap = SimulatedGdspStrap({1: [(T0, ACT, False)]})
        c, _ = _client(strap)
        f = await c.fetch(1, T0 - 60, timezone.utc)
        assert isinstance(f, h.FetchChunk) and f.payload == ACT and f.header_time == T0
        assert strap.acks == [1] and strap.starts == [(1, T0 - 60)]
    run(go())


def test_compressed_type_87_is_inflated_and_parsed():
    body = ByteWriter().u32(T0).u16(4).u8(60).u8(61).u8(0xFF).u8(0xFF).u32(T0 + 10).u16(2).u8(70).u8(71).to_bytes()

    async def go():
        strap = SimulatedGdspStrap({87: [(T0, body, True)]})
        c, _ = _client(strap)
        res = await h.sync_type(c, h.SPECS_BY_CODE[87], T0 - 5, timezone.utc)
        assert res.ok
        assert [(r.timestamp, r.heart_rate) for r in res.records] == [(T0, 60), (T0 + 1, 61), (T0 + 10, 70),
                                                                      (T0 + 11, 71)]
        # second START from the last sample + 1 s finds nothing
        assert strap.starts == [(87, T0 - 5), (87, T0 + 12)]
        assert strap.acks == [1, 1]
    run(go())


def test_sync_type_loops_activity_with_size_field_as_record_count():
    async def go():
        strap = SimulatedGdspStrap({1: [(T0, ACT, False), (T0 + 480, ACT, False)]})
        strap.size_as_records = 8  # the strap counts records in the size field
        c, _ = _client(strap)
        res = await h.sync_type(c, h.SPECS_BY_CODE[1], T0, timezone.utc)
        assert res.ok and len(res.records) == 16
        assert res.records[8].timestamp == T0 + 480
        assert [s[1] for s in strap.starts] == [T0, T0 + 480, T0 + 960]
        assert h.saved_cursor(1, res.records, res.next) == T0 + 960
    run(go())


def test_fast_recharge_is_acknowledged_with_33():
    async def go():
        strap = SimulatedGdspStrap({})
        strap.fast_recharge = True
        c, _ = _client(strap)
        f = await c.fetch(1, T0, timezone.utc)
        assert isinstance(f, h.FetchFailed) and f.fast_recharge
        assert strap.acks == [0x33]
    run(go())


def test_data_available_push_is_reported():
    async def go():
        strap = SimulatedGdspStrap({1: [(T0, ACT, False)]})
        strap.push_before_start = 5
        pushes = []
        c, _ = _client(strap, on_data_available=pushes.append)
        assert isinstance(await c.fetch(1, T0, timezone.utc), h.FetchChunk)
        assert pushes[0].types_to_sync == [5, 6, 79]
    run(go())


def test_failed_version_query_is_logged_and_reads_as_none():
    async def go():
        async def broken(b):
            raise RuntimeError("link lost")
        logs = []
        c = h.GdspClient(broken, log=logs.append, timeout=0.2)
        assert await c.query_version() is None
        assert any("version query failed" in s and "link lost" in s for s in logs)
    run(go())


def test_unanswered_start_is_acknowledged_with_04():
    async def go():
        strap = SimulatedGdspStrap({})
        strap.silent_starts = 1
        c, logs = _client(strap)
        f = await c.fetch(1, T0, timezone.utc)
        assert isinstance(f, h.FetchFailed) and f.ack == h.GdspStatus.OPERATION_FAILED
        assert any(s.startswith("history: start 1: ") and "no reply" in s for s in logs)
        assert isinstance(await c.fetch(1, T0, timezone.utc), h.FetchEmpty)
        assert strap.acks == [4, 1]
    run(go())


def test_refused_unknown_and_empty_starts():
    async def go():
        strap = SimulatedGdspStrap({})
        strap.invalid_state_starts = 1
        c, _ = _client(strap)
        f = await c.fetch(1, T0, timezone.utc)
        assert f.ack == h.GdspStatus.INVALID_STATE and len(strap.starts) == 1
        strap.next_status = 0x77  # unknown status counts as success
        assert isinstance(await c.fetch(1, T0, timezone.utc), h.FetchEmpty)
        assert isinstance(await c.fetch(1, T0, timezone.utc), h.FetchEmpty)
        assert strap.acks == [5, 1, 1]
    run(go())


def test_missing_repeated_and_bad_crc_packets():
    async def go():
        strap = SimulatedGdspStrap({1: [(T0, ACT, False)]})
        c, _ = _client(strap)
        strap.drop_packet = 2
        assert (await c.fetch(1, T0, timezone.utc)).ack == h.GdspStatus.PACKET_INDEX_ERROR
        strap.double_packets = True
        ok = await c.fetch(1, T0, timezone.utc)
        assert isinstance(ok, h.FetchChunk) and ok.payload == ACT
        strap.bad_crc = True
        assert (await c.fetch(1, T0, timezone.utc)).ack == h.GdspStatus.CRC_MISMATCH
        assert strap.acks == [6, 1, 8]
    run(go())


def test_header_not_later_after_the_cursor_moved_is_cyclic():
    async def go():
        strap = SimulatedGdspStrap({1: [(T0, ACT, False)]})
        strap.header = T0
        c, _ = _client(strap)
        f = await c.fetch(1, T0 - 60, timezone.utc, h.PreviousChunk(T0 - 120, T0))
        assert isinstance(f, h.FetchFailed) and f.cyclic
        assert strap.acks == [0xFA]
        same = await c.fetch(1, T0 - 60, timezone.utc, h.PreviousChunk(T0 - 60, T0))
        assert isinstance(same, h.FetchChunk)
    run(go())


def test_cursor_helpers():
    zone = timezone.utc
    now = 1_792_141_453
    d = h.default_cursor(now, zone)
    assert d % 43200 == 0 and now - 3 * 86400 < d <= now - 2 * 86400
    assert h.cursor_on_read(5, now) == h.CURSOR_FLOOR
    assert h.cursor_on_read(now + 7200, now) == now - 2 * 86400
    assert h.start_cursor(37, now, zone, lambda c: 1000) == 1001
    assert h.start_cursor(72, now, zone, lambda c: 1000) == now - 45 * 86400
