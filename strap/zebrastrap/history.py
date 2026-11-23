"""GDSP history sync: codec, client and record parsers (FetchCodec.kt, GdspClient.kt, RecordParsers.kt).

Control commands travel over channel endpoint 0x004B. Bulk data arrives as indexed notifications on HUAMI(0005).
"""

from __future__ import annotations

import asyncio
import calendar
import dataclasses
import struct
import zlib
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone, tzinfo
from typing import Callable, Optional, Union

from ._bytes import ByteReader, ByteWriter, ShortRead, u32
from .crypto import crc32

# ============================================================================================== time


@dataclass(frozen=True)
class GdspTime:
    """Wall-clock time on the GDSP wire: local date fields plus the UTC offset in quarter-hours (signed)."""

    year: int
    month: int
    day: int
    hour: int
    minute: int
    second: int
    tz_quarters: int

    WIRE_SIZE = 8

    def to_epoch_second(self) -> int:
        naive = datetime(self.year, self.month, self.day, self.hour, self.minute, self.second, tzinfo=timezone.utc)
        return int(naive.timestamp()) - self.tz_quarters * 900

    def write_to(self, w: ByteWriter) -> None:
        w.u16(self.year).u8(self.month).u8(self.day).u8(self.hour).u8(self.minute).u8(self.second).u8(self.tz_quarters)

    @staticmethod
    def of_datetime(t: datetime) -> "GdspTime":
        off = int(t.utcoffset().total_seconds())  # type: ignore[union-attr]
        quarters = abs(off) // 900 * (1 if off >= 0 else -1)  # truncates toward zero, as Kotlin Int division
        return GdspTime(t.year, t.month, t.day, t.hour, t.minute, t.second, quarters)

    @staticmethod
    def of(epoch_second: int, zone: tzinfo) -> "GdspTime":
        return GdspTime.of_datetime(datetime.fromtimestamp(epoch_second, zone))

    @staticmethod
    def read(r: ByteReader) -> "GdspTime":
        return GdspTime(r.u16(), r.u8(), r.u8(), r.u8(), r.u8(), r.u8(), r.s8())


# ============================================================================================== status


class GdspStatus:
    SUCCESS = 1
    UNKNOWN_COMMAND = 2
    INVALID_PARAMETER = 3
    OPERATION_FAILED = 4
    INVALID_STATE = 5
    PACKET_INDEX_ERROR = 6
    BUSY = 7
    CRC_MISMATCH = 8
    CANCELLED_BY_APP = 9
    CONTENT_PARSE_ERROR = 10
    UNAUTHORIZED = 11
    COMPRESS_FAILED = 12
    DECOMPRESS_FAILED = 13
    DATA_SIZE_MISMATCH = 14
    MUSIC_RUNNING = 48
    SPEECH_RUNNING = 49
    TIMEOUT = 50
    FAST_RECHARGE_MODE = 51
    UNKNOWN = -1

    NAMES: dict = {}

    @classmethod
    def from_code(cls, code: int) -> int:
        return code if code in cls.NAMES and code != cls.UNKNOWN else cls.UNKNOWN

    @classmethod
    def name(cls, code: int) -> str:
        return cls.NAMES.get(cls.from_code(code), "UNKNOWN")


GdspStatus.NAMES = {v: k for k, v in vars(GdspStatus).items() if isinstance(v, int) and k.isupper()}

# ============================================================================================== messages


@dataclass
class Response:
    opcode: int
    status_code: int

    @property
    def status(self) -> int:
        return GdspStatus.from_code(self.status_code)

    @property
    def is_success(self) -> bool:
        return self.status_code == GdspStatus.SUCCESS


@dataclass
class VersionReply(Response):
    version: Optional[int] = None
    raw_data: bytes = b""

    def supported_types(self) -> Optional[set[int]]:
        """`version u8, supportDataUpload u8, count u16, count x {type u8, hasData u8} [supportQueryDataStatus]`."""
        if len(self.raw_data) < 4:
            return None
        r = ByteReader(self.raw_data, 2)
        count = r.u16()
        if r.remaining < count * 2:
            return None
        out = set()
        for _ in range(count):
            out.add(r.u8())
            r.u8()
        return out


@dataclass
class StartReply(Response):
    size: int = 0
    first_record_time: Optional[GdspTime] = None
    expansion: bytes = b""

    @property
    def first_record_epoch(self) -> Optional[int]:
        return self.first_record_time.to_epoch_second() if self.first_record_time else None

    @property
    def is_compressed(self) -> Optional[bool]:
        """Expansion byte 0 is the compression flag: 1 = zlib payload. None when no expansion was sent."""
        return (self.expansion[0] == 1) if self.expansion else None


@dataclass
class TransferComplete(Response):
    extra: bytes = b""

    @property
    def crc32(self) -> Optional[int]:
        return u32(self.extra, 0) if len(self.extra) >= 4 else None


@dataclass
class AckReply(Response):
    data: bytes = b""


@dataclass
class OtherReply(Response):
    data: bytes = b""


@dataclass
class DataAvailable:
    type_code: int

    @property
    def types_to_sync(self) -> list[int]:
        if self.type_code in (5, 6):
            return [5, 6, 79]
        if self.type_code in (70, 96):
            return [self.type_code]
        return []


@dataclass
class Malformed:
    reason: str
    raw: bytes


GdspMessage = Union[Response, DataAvailable, Malformed]

# ============================================================================================== codec

OP_QUERY_VERSION = 0x00
OP_START = 0x01
OP_TRANSFER = 0x02
OP_ACK = 0x03
OP_QUERY_DATA_STATUS = 0x05
RESPONSE_FLAG = 0x10
NOTIFY_DATA_AVAILABLE = 0x11
START_FLAG_HEALTH = 0x01
CHANNEL_MODULE = 0x004B


def query_version() -> bytes:
    return bytes([OP_QUERY_VERSION])


def query_data_status() -> bytes:
    return bytes([OP_QUERY_DATA_STATUS])


def transfer() -> bytes:
    return bytes([OP_TRANSFER])


def ack(param: Optional[int] = None) -> bytes:
    return bytes([OP_ACK]) if param is None else bytes([OP_ACK, param & 0xFF])


def start_fetch(type_code: int, since: Union[GdspTime, datetime], flag: Optional[int] = START_FLAG_HEALTH) -> bytes:
    """START: `01 type Y(2) M D h m s tz [flag]`."""
    if isinstance(since, datetime):
        since = GdspTime.of_datetime(since)
    w = ByteWriter().u8(OP_START).u8(type_code)
    since.write_to(w)
    if flag is not None:
        w.u8(flag)
    return w.to_bytes()


def start_fetch_epoch(type_code: int, epoch: int, zone: tzinfo, flag: Optional[int] = START_FLAG_HEALTH) -> bytes:
    return start_fetch(type_code, GdspTime.of(epoch, zone), flag)


def parse(b: bytes) -> GdspMessage:
    """Decode one control-path message (channel 0x004B payload)."""
    b = bytes(b)
    if not b:
        return Malformed("empty", b)
    if b[0] == NOTIFY_DATA_AVAILABLE:
        return DataAvailable(b[1]) if len(b) >= 2 else Malformed("data-available notification without type", b)
    if b[0] == RESPONSE_FLAG:
        return _parse_response(b)
    return Malformed("unknown leading byte", b)


def _parse_response(b: bytes) -> GdspMessage:
    if len(b) < 3:
        return Malformed("response shorter than 3 bytes", b)
    op, status, data = b[1], b[2], b[3:]
    if op == OP_QUERY_VERSION:
        return VersionReply(op, status, data[0] if data else None, data)
    if op == OP_START:
        if status != GdspStatus.SUCCESS:
            return StartReply(op, status, 0, None, data)
        if len(data) < 4 + GdspTime.WIRE_SIZE:
            return Malformed(f"START reply too short ({len(data)} data bytes)", b)
        r = ByteReader(data)
        size = r.u32()
        t = GdspTime.read(r)
        return StartReply(op, status, size, t, r.rest())
    if op == OP_TRANSFER:
        return TransferComplete(op, status, data)
    if op == OP_ACK:
        return AckReply(op, status, data)
    return OtherReply(op, status, data)


def inflate(b: bytes) -> bytes:
    d = zlib.decompressobj()
    out = d.decompress(b)
    if not d.eof:
        raise ValueError("truncated zlib stream")
    return out


def is_zlib(b: bytes) -> bool:
    return len(b) >= 2 and (b[0] & 0x0F) == 8 and (b[0] * 256 + b[1]) % 31 == 0


# ============================================================================================== client

CYCLIC_SYNC = -6
SIZE_UNCHECKED = {60, 80, 84}


class GdspTimeout(Exception):
    def __init__(self, op: int) -> None:
        super().__init__(f"no reply to GDSP command {op:02x}")


@dataclass
class FetchEmpty:
    ack: int = GdspStatus.SUCCESS


@dataclass
class FetchFailed:
    ack: int
    reason: str

    @property
    def cyclic(self) -> bool:
        return self.ack == CYCLIC_SYNC

    @property
    def fast_recharge(self) -> bool:
        return self.ack == GdspStatus.FAST_RECHARGE_MODE


@dataclass
class FetchChunk:
    payload: bytes
    header_time: int
    size_field: int
    ack: int = GdspStatus.SUCCESS

    @property
    def unit(self) -> Optional[int]:
        """Bytes per unit of size_field: 1 when it counts bytes, None when the payload does not fit it."""
        if len(self.payload) == self.size_field:
            return 1
        if self.size_field > 0 and len(self.payload) % self.size_field == 0:
            return len(self.payload) // self.size_field
        return None


GdspFetch = Union[FetchEmpty, FetchFailed, FetchChunk]


@dataclass(frozen=True)
class PreviousChunk:
    cursor: int
    header_time: int


class GdspClient:
    """The phone side of a GDSP fetch, as Zepp 10.10.5 runs it.

    send writes one control command (to channel 0x004B). Feed control payloads to on_control() and
    HUAMI(0005) packets to on_data(). Both go into one queue, so their arrival order is kept.
    """

    def __init__(self, send: Callable, log: Callable[[str], None] = lambda s: None,
                 on_data_available: Optional[Callable[[DataAvailable], None]] = None,
                 timeout: float = 20.0, trailing: float = 0.3) -> None:
        self.send = send
        self.log = log
        self.on_data_available = on_data_available or (
            lambda m: log(f"history: device has new data of type {m.type_code}"))
        self.timeout = timeout
        self.trailing = trailing
        self._q: asyncio.Queue = asyncio.Queue()

    # ---- inputs
    def on_control(self, payload: bytes) -> None:
        self._q.put_nowait(("c", parse(payload)))

    def on_control_message(self, msg: GdspMessage) -> None:
        self._q.put_nowait(("c", msg))

    def on_data(self, packet: bytes) -> None:
        self._q.put_nowait(("d", bytes(packet)))

    def _drop_data(self) -> None:
        keep = []
        while not self._q.empty():
            item = self._q.get_nowait()
            if item[0] == "c":
                keep.append(item)
        for item in keep:
            self._q.put_nowait(item)

    async def _control(self, timeout: float):
        """The next control message, skipping data packets. Raises asyncio.TimeoutError."""
        async with asyncio.timeout(timeout):
            while True:
                kind, item = await self._q.get()
                if kind == "c":
                    return item

    async def command(self, b: bytes, timeout: Optional[float] = None) -> Response:
        op = b[0]
        await self.send(b)
        try:
            async with asyncio.timeout(self.timeout if timeout is None else timeout):
                while True:
                    kind, m = await self._q.get()
                    if kind != "c":
                        continue
                    if isinstance(m, Response) and m.opcode == op:
                        return m
                    if isinstance(m, DataAvailable):
                        self.on_data_available(m)
        except TimeoutError:
            raise GdspTimeout(op) from None

    async def query_version(self) -> Optional[VersionReply]:
        try:
            r = await self.command(query_version())
        except Exception as e:
            self.log(f"history: version query failed: {e}")
            return None
        return r if isinstance(r, VersionReply) else None

    async def fetch(self, type_code: int, since: int, zone: tzinfo,
                    previous: Optional[PreviousChunk] = None) -> GdspFetch:
        """One START to ACK exchange. The ACK `03 <code>` goes out in every case, before anything is parsed."""
        try:
            start = await self.command(start_fetch_epoch(type_code, since, zone))
        except GdspTimeout as e:
            self.log(f"history: start {type_code}: {e}")
            return await self._closed(FetchFailed(GdspStatus.OPERATION_FAILED, "no reply to START"))
        known = GdspStatus.from_code(start.status_code) != GdspStatus.UNKNOWN
        if known and not start.is_success:
            return await self._closed(FetchFailed(start.status_code,
                                                  f"start refused ({GdspStatus.name(start.status_code)})"))
        if not isinstance(start, StartReply):
            return await self._closed(FetchFailed(GdspStatus.OPERATION_FAILED, "no header"))
        if start.size == 0:
            return await self._closed(FetchEmpty())
        header = start.first_record_epoch
        if header is None:
            return await self._closed(FetchFailed(GdspStatus.OPERATION_FAILED, "no header"))
        if previous is not None and since != previous.cursor and header <= previous.header_time:
            return await self._closed(FetchFailed(CYCLIC_SYNC,
                                                  f"cyclic: header {header} not after {previous.header_time}"))
        return await self._closed(await self._transfer(type_code, start, header))

    async def _transfer(self, type_code: int, start: StartReply, header: int) -> GdspFetch:
        self._drop_data()  # packets left over from an earlier transfer
        wire = bytearray()
        last_index = -1
        last_packet: Optional[bytes] = None
        await self.send(transfer())
        complete: Optional[TransferComplete] = None
        stash: list = []
        while complete is None:
            try:
                async with asyncio.timeout(self.timeout):
                    kind, got = await self._q.get()
            except TimeoutError:
                self.log("history: no data for 20 s")
                return FetchFailed(GdspStatus.OPERATION_FAILED, "no data for 20 s")
            if kind == "d":
                if not got:
                    continue
                index = got[0]
                if index == last_index and got == last_packet:
                    continue  # the same packet again: Zepp ignores it
                if index == ((last_index + 1) & 0xFF) and (last_index >= 0 or index == 0):
                    last_index = index
                    last_packet = got
                    wire += got[1:]
                else:
                    self.log(f"history: missing packet, {last_index} then {index}")
                    return FetchFailed(GdspStatus.PACKET_INDEX_ERROR, f"packet gap {last_index} -> {index}")
            elif isinstance(got, TransferComplete):
                complete = got
            elif isinstance(got, DataAvailable):
                self.on_data_available(got)
            elif isinstance(got, Malformed):
                self.log(f"history: unexpected during transfer: {got.reason} {got.raw.hex()}")
            else:
                self.log(f"history: unexpected during transfer: {got}")
        # Data packets can arrive just after the `10 02` on the control path.
        try:
            async with asyncio.timeout(self.trailing):
                while True:
                    kind, p = await self._q.get()
                    if kind == "c":
                        stash.append((kind, p))
                    elif p and p[0] == ((last_index + 1) & 0xFF):
                        last_index = p[0]
                        wire += p[1:]
        except TimeoutError:
            pass
        for item in stash:
            self._q.put_nowait(item)
        raw = bytes(wire)
        if start.is_compressed is True:
            try:
                payload = inflate(raw)
            except Exception as e:
                return FetchFailed(GdspStatus.DECOMPRESS_FAILED, f"inflate failed: {e}")
        else:
            payload = raw
        if len(payload) < start.size and type_code not in SIZE_UNCHECKED:
            return FetchFailed(GdspStatus.DATA_SIZE_MISMATCH, f"{len(payload)} bytes for size {start.size}")
        if not complete.is_success or len(complete.extra) != 4:
            return FetchFailed(GdspStatus.OPERATION_FAILED,
                               f"transfer ended with {complete.status_code} {complete.extra.hex()}")
        if complete.crc32 != crc32(raw):
            return FetchFailed(GdspStatus.CRC_MISMATCH, "CRC mismatch")
        return FetchChunk(payload, header, start.size)

    async def _closed(self, result: GdspFetch) -> GdspFetch:
        """The ACK every exchange ends with, `03 <code>`, then up to 20 s for the strap's `10 03`."""
        await self.send(ack(result.ack))
        reply = None
        try:
            async with asyncio.timeout(self.timeout):
                while True:
                    kind, m = await self._q.get()
                    if kind != "c":
                        continue
                    if isinstance(m, Response) and m.opcode == OP_ACK:
                        reply = m
                        break
                    if isinstance(m, DataAvailable):
                        self.on_data_available(m)
        except TimeoutError:
            pass
        self.log(f"history: ack {result.ack & 0xFF:02x} -> {GdspStatus.name(reply.status_code) if reply else 'no reply'}")
        return result


# ============================================================================================== records

HR_VALID = range(30, 221)


def valid_hr(raw: int) -> Optional[int]:
    return raw if raw in HR_VALID else None


class ActivityMode:
    SILENT = 0
    WALKING = 1
    RUNNING = 2
    NOT_WORN = 3
    REM = 4
    NREM = 5
    CHARGING = 6
    ON_BED = 7
    DREAM = 8
    USER_SLEEP = 9
    BLANK = 126


@dataclass(frozen=True)
class ActivitySleepExtra:
    retain: int
    sleep_posture_and_prob: int
    sleep_prob_deep: int
    sleep_prob_rem: int


@dataclass(frozen=True)
class ActivitySample:
    """Type 1, one per minute. Low nibble of byte 0 is the ActivityMode."""

    timestamp: int
    raw_mode: int
    intensity: int
    steps: int
    raw_heart_rate: int
    sleep_extra: Optional[ActivitySleepExtra]

    @property
    def heart_rate(self) -> Optional[int]:
        return valid_hr(self.raw_heart_rate)

    @property
    def mode(self) -> int:
        return self.raw_mode if self.raw_mode == ActivityMode.BLANK else self.raw_mode & 0x0F

    @property
    def is_worn(self) -> bool:
        return self.mode not in (ActivityMode.NOT_WORN, ActivityMode.CHARGING)

    @property
    def is_blank(self) -> bool:
        vals = [self.raw_mode, self.intensity, self.steps, self.raw_heart_rate]
        if self.sleep_extra:
            e = self.sleep_extra
            vals += [e.retain, e.sleep_posture_and_prob, e.sleep_prob_deep, e.sleep_prob_rem]
        return all(v == 0xFF for v in vals)


@dataclass(frozen=True)
class SecondHeartRate:
    """Types 76 and 87: one HR sample per second."""

    timestamp: int
    raw_heart_rate: int

    @property
    def heart_rate(self) -> Optional[int]:
        return valid_hr(self.raw_heart_rate)


class SleepStageCodes:
    LIGHT = 4
    DEEP = 5
    AWAKE = 7
    REM = 8
    VALID = {4, 5, 7, 8}
    NAMES = {4: "LIGHT", 5: "DEEP", 7: "AWAKE", 8: "REM"}


@dataclass(frozen=True)
class SleepStage:
    start: int
    stop: int
    raw_type: int

    @property
    def type(self) -> str:
        return SleepStageCodes.NAMES.get(self.raw_type, "UNKNOWN")


@dataclass(frozen=True)
class ShortSleep:
    start: int
    stop: int
    length: int


@dataclass(frozen=True)
class SleepResult:
    """Type 72: on-device sleep result (594 bytes). i16 time fields are raw minutes."""

    timestamp: int
    belong_time: int
    version: int
    has_main_sleep: bool
    main_sleep_start: int
    main_sleep_end: int
    on_bed_time: int
    off_bed_time: int
    is_rem_supported: bool
    is_short_sleep_supported: bool
    has_short_sleep: bool
    average_heart_rate: int
    sleep_score: int
    short_sleeps: list
    main_stages: list
    short_stages: list
    rem_sleep_length: int
    light_sleep_length: int
    deep_sleep_length: int
    wake_length: int

    def day(self, zone: tzinfo) -> int:
        """Local midnight of the day this result belongs to: the date of belong_time + 2 h."""
        d = datetime.fromtimestamp(self.belong_time + 2 * 3600, zone).date()
        return int(datetime(d.year, d.month, d.day, tzinfo=zone).timestamp())

    def resolve_time(self, raw: int, zone: tzinfo) -> int:
        """Raw times are minutes from local midnight of the day before day(), so 1440 = midnight of day()."""
        return self.day(zone) + (raw - 1440) * 60

    @property
    def main_sleep_end_fixed(self) -> int:
        return self.main_sleep_end + 1 if self.version == 0 else self.main_sleep_end

    @property
    def total_sleep_minutes(self) -> int:
        return self.rem_sleep_length + self.light_sleep_length + self.deep_sleep_length

    @property
    def wake_count(self) -> int:
        return sum(1 for s in self.main_stages if s.raw_type == SleepStageCodes.AWAKE)


@dataclass(frozen=True)
class SpO2Reading:
    """Type 37: spot/auto SpO2. history holds the 60 preceding samples (v2 only)."""

    timestamp: int
    spo2: int
    is_auto: bool
    history: Optional[list]

    @property
    def is_plausible(self) -> bool:
        return 80 <= self.spo2 <= 100


@dataclass(frozen=True)
class SleepSpO2Extra:
    high: bytes
    low: bytes
    signal_quality: bytes
    extend: bytes


@dataclass(frozen=True)
class SleepSpO2Sample:
    """Type 38: sleep SpO2 samples."""

    timestamp: int
    spo2: int
    time: int
    v2: Optional[SleepSpO2Extra]


@dataclass(frozen=True)
class SpO2OsaEvent:
    """Type 39: OSA desaturation event. spo2/heart_rate (240 each) only in the long form."""

    timestamp: int
    spo2_decrease: int
    spo2: Optional[list]
    heart_rate: Optional[list]


@dataclass(frozen=True)
class StressSample:
    """Types 18 (single) and 19 (all-day, per minute)."""

    timestamp: int
    stress: int
    all_day: bool

    @property
    def is_likely_gap(self) -> bool:
        return self.stress == 0 or self.stress >= 254


@dataclass(frozen=True)
class SkinTemperatureSample:
    """Type 46: per-minute temperatures in 0.01 C. None when the sensor had no reading."""

    timestamp: int
    skin_centi_celsius: Optional[int]
    ambient_centi_celsius: Optional[int]

    @property
    def skin_celsius(self) -> Optional[float]:
        if self.skin_centi_celsius is None:
            return None
        c = self.skin_centi_celsius / 100
        return c if 20.0 <= c <= 42.0 else None


@dataclass(frozen=True)
class HrvRmssd:
    timestamp: int
    tz_quarters: int
    rmssd: int


@dataclass(frozen=True)
class HrvSdnn:
    """Type 78: nightly SDNN in 0.01 ms."""

    timestamp: int
    tz_quarters: int
    sdnn_centi_ms: int

    @property
    def sdnn_ms(self) -> float:
        return self.sdnn_centi_ms / 100.0


@dataclass(frozen=True)
class RestingHeartRate:
    timestamp: int
    tz_quarters: int
    heart_rate: int


@dataclass(frozen=True)
class DailyMaxHeartRate:
    timestamp: int
    tz_quarters: int
    max_heart_rate: int


@dataclass(frozen=True)
class SleepBreathRate:
    timestamp: int
    tz_quarters: int
    breath_rate: int
    snr: int
    channel: int


# ============================================================================================== parsers


@dataclass(frozen=True)
class ParseContext:
    """header_time: epoch seconds of the START reply time. activity_record_size: 4 or 8.
    spo2_versioned: None means detect from the payload."""

    header_time: int
    activity_record_size: int = 4
    spo2_versioned: Optional[bool] = None


@dataclass
class ParseSuccess:
    records: list
    next_cursor: Optional[int]
    dropped_bytes: int = 0


@dataclass
class ParseFailure:
    reason: str


ParseResult = Union[ParseSuccess, ParseFailure]


def _guarded(fn):
    def wrapper(payload: bytes, ctx: ParseContext) -> ParseResult:
        try:
            return fn(bytes(payload), ctx)
        except ShortRead as e:
            return ParseFailure(f"truncated payload: {e}")
        except ValueError as e:
            return ParseFailure(f"invalid payload: {e}")
    wrapper.__name__ = fn.__name__
    return wrapper


def _last_plus_one(records: list) -> Optional[int]:
    return max(r.timestamp for r in records) + 1 if records else None


def _fixed(payload: bytes, size: int, strict: bool, read, offset: int = 0, nxt=_last_plus_one) -> ParseResult:
    body = len(payload) - offset
    if body < 0:
        return ParseFailure("payload shorter than header")
    rem = body % size
    if strict and rem:
        return ParseFailure(f"length {body} not a multiple of {size}")
    r = ByteReader(payload, offset)
    out = []
    i = 0
    while r.remaining >= size:
        start = r.pos
        out.append(read(r, i))
        i += 1
        r.pos = start + size
    return ParseSuccess(out, nxt(out), rem)


NEAR_HEADER_SECONDS = 2 * 86_400


def _is_spo2_versioned(payload: bytes, ctx: ParseContext) -> bool:
    """Without the feature bit: byte 0 is a known version and the first record's time sits right after it."""
    if ctx.spo2_versioned is not None:
        return ctx.spo2_versioned
    if len(payload) < 5 or payload[0] not in (1, 2):
        return False

    def near(at: int) -> bool:
        return abs(u32(payload, at) - ctx.header_time) <= NEAR_HEADER_SECONDS

    return near(1) and not near(0)


def _version_of(payload: bytes, ctx: ParseContext):
    """(version or None, offset), or None for an empty versioned payload."""
    if not _is_spo2_versioned(payload, ctx):
        return None, 0
    if not payload:
        return None
    return payload[0], 1


def _u8_list(r: ByteReader, n: int) -> list:
    return list(r.bytes(n))


@_guarded
def parse_activity(raw: bytes, ctx: ParseContext) -> ParseResult:
    """Type 1: per-minute activity, 4-byte or >= 8-byte records. Blank (all 0xFF) slots count as minutes."""
    size = 4 if ctx.activity_record_size <= 4 else ctx.activity_record_size

    def read(r: ByteReader, i: int) -> ActivitySample:
        mode, intensity, steps, hr = r.u8(), r.u8(), r.u8(), r.u8()
        extra = ActivitySleepExtra(r.u8(), r.u8(), r.u8(), r.u8()) if size >= 8 else None
        return ActivitySample(ctx.header_time + i * 60, mode, intensity, steps, hr, extra)

    return _fixed(raw, size, False, read, nxt=lambda recs: ctx.header_time + len(recs) * 60 if recs else None)


@_guarded
def parse_second_hr(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 76: `ts u32, hr u8`."""
    return _fixed(payload, 5, False, lambda r, i: SecondHeartRate(r.u32(), r.u8()))


@_guarded
def parse_second_hr_v2(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 87: blocks `start_ts u32, count u16, hr[count]`, trailing 0xFF trimmed."""
    r = ByteReader(payload)
    out = []
    while r.remaining >= 6:
        start = r.u32()
        count = r.u16()
        if count == 0:
            continue
        if r.remaining < count:
            return ParseFailure("SEC_HR_V2 block truncated")
        hrs = r.bytes(count)
        n = count
        while n > 0 and hrs[n - 1] == 0xFF:
            n -= 1
        out += [SecondHeartRate(start + k, hrs[k]) for k in range(n)]
    # A short block sometimes repeats the next block's first second: keep the later value.
    unique = {}
    for s in out:
        unique[s.timestamp] = s
    recs = sorted(unique.values(), key=lambda s: s.timestamp)
    return ParseSuccess(recs, _last_plus_one(recs), r.remaining)


SLEEP_RESULT_SIZE = 594


@_guarded
def parse_sleep_result(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 72: 594-byte on-device sleep result."""

    def read(r: ByteReader, i: int) -> SleepResult:
        ts, belong, version = r.u32(), r.u32(), r.u8()
        has_main = r.u8() == 1
        main_start, main_end, on_bed, off_bed = r.s16(), r.s16(), r.s16(), r.s16()
        rem_sup, short_sup, has_short = r.u8() == 1, r.u8() == 1, r.u8() == 1
        ave_hr, score = r.u8(), r.u8()
        short_count = min(max(r.s8(), 0), 10)
        shorts = [ShortSleep(r.s16(), r.s16(), r.s16()) for _ in range(10)][:short_count]
        main_count = min(max(r.s8(), 0), 50)
        r.s8()  # nap stage count: Zepp ignores it
        main = [SleepStage(r.s16(), r.s16(), r.u8()) for _ in range(50)][:main_count]
        naps = [s for s in (SleepStage(r.s16(), r.s16(), r.u8()) for _ in range(50))
                if s.raw_type in SleepStageCodes.VALID]
        return SleepResult(ts, belong, version, has_main, main_start, main_end, on_bed, off_bed, rem_sup, short_sup,
                           has_short, ave_hr, score, shorts, main, naps, r.s16(), r.s16(), r.s16(), r.s16())

    return _fixed(payload, SLEEP_RESULT_SIZE, False, read)


@_guarded
def parse_spo2_single(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 37: bit 7 = isAuto. v2 adds 60 history samples."""
    vo = _version_of(payload, ctx)
    if vo is None:
        return ParseSuccess([], None)
    v, off = vo
    if v in (None, 1):
        def read(r, i):
            ts, b = r.u32(), r.u8()
            return SpO2Reading(ts, b & 0x7F, bool(b & 0x80), None)
        return _fixed(payload, 5, False, read, offset=off)
    if v == 2:
        def read2(r, i):
            ts, b = r.u32(), r.u8()
            return SpO2Reading(ts, b & 0x7F, bool(b & 0x80), _u8_list(r, 60))
        return _fixed(payload, 65, False, read2, offset=off)
    return ParseFailure(f"SPO2 version {v} not supported")


@_guarded
def parse_spo2_sleep(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 38: sleep SpO2 samples."""
    vo = _version_of(payload, ctx)
    if vo is None:
        return ParseSuccess([], None)
    v, off = vo
    if v in (None, 1):
        return _fixed(payload, 6, False, lambda r, i: SleepSpO2Sample(r.u32(), r.u8(), r.u8(), None), offset=off)
    if v == 2:
        return _fixed(payload, 30, False, lambda r, i: SleepSpO2Sample(
            r.u32(), r.u8(), r.u8(), SleepSpO2Extra(r.bytes(6), r.bytes(6), r.bytes(8), r.bytes(4))), offset=off)
    return ParseFailure(f"SPO2 sleep version {v} not supported")


@_guarded
def parse_spo2_osa_event(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 39: no version byte or v=2: 485 B. v=1: 5 B."""
    vo = _version_of(payload, ctx)
    if vo is None:
        return ParseSuccess([], None)
    v, off = vo
    if v in (None, 2):
        return _fixed(payload, 485, False, lambda r, i: SpO2OsaEvent(r.u32(), r.u8(), _u8_list(r, 240),
                                                                    _u8_list(r, 240)), offset=off)
    if v == 1:
        return _fixed(payload, 5, False, lambda r, i: SpO2OsaEvent(r.u32(), r.u8(), None, None), offset=off)
    return ParseFailure(f"OSA event version {v} not supported")


@_guarded
def parse_stress_single(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 18: `ts u32, stress u8`. Rejected if not a multiple of 5."""
    return _fixed(payload, 5, True, lambda r, i: StressSample(r.u32(), r.u8(), False))


@_guarded
def parse_stress_all_day(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 19: one byte per minute. Cursor = header + N min + 1 s."""
    return _fixed(payload, 1, False, lambda r, i: StressSample(ctx.header_time + i * 60, r.u8(), True),
                  nxt=lambda recs: ctx.header_time + len(recs) * 60 + 1 if recs else None)


TEMP_INVALID = 0x7FFF


@_guarded
def parse_skin_temperature(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 46: 8-byte records per minute: `env s16, touch s16, reserved[4]`, 0.01 C, 0x7FFF = none."""

    def read(r, i):
        env, touch = r.s16(), r.s16()
        r.bytes(4)
        return SkinTemperatureSample(ctx.header_time + i * 60, None if touch == TEMP_INVALID else touch,
                                     None if env == TEMP_INVALID else env)

    return _fixed(payload, 8, False, read, nxt=lambda recs: ctx.header_time + len(recs) * 60 + 1 if recs else None)


@_guarded
def parse_hrv_rmssd(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 73: `ts u32, tz i8, rmssd u8`."""
    return _fixed(payload, 6, False, lambda r, i: HrvRmssd(r.u32(), r.s8(), r.u8()))


@_guarded
def parse_hrv_sdnn(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 78: `ts u32, tz i8, sdnn u32` in 0.01 ms."""
    return _fixed(payload, 9, False, lambda r, i: HrvSdnn(r.u32(), r.s8(), r.u32()))


@_guarded
def parse_resting_hr(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 58: `ts, tz, hr`."""
    return _fixed(payload, 6, False, lambda r, i: RestingHeartRate(r.u32(), r.s8(), r.u8()))


@_guarded
def parse_daily_max_hr(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 61: `ts, tz, maxHr`."""
    return _fixed(payload, 6, False, lambda r, i: DailyMaxHeartRate(r.u32(), r.s8(), r.u8()))


@_guarded
def parse_sleep_breath_rate(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 56: `ts, tz, breathRate, snr, channel`."""
    return _fixed(payload, 8, False, lambda r, i: SleepBreathRate(r.u32(), r.s8(), r.u8(), r.u8(), r.u8()))


# ---- more record types (RecordParsers.kt)


@dataclass(frozen=True)
class SpotHeartRate:
    """Type 2: manual / spot HR."""

    timestamp: int
    tz_quarters: int
    raw_heart_rate: int

    @property
    def heart_rate(self) -> Optional[int]:
        return valid_hr(self.raw_heart_rate)


@dataclass(frozen=True)
class TodayActivityExtended:
    """Type 88, one per minute."""

    timestamp: int
    version: int
    distance_type: int
    floor_count: int
    run_duration: int
    fat_burn_duration: int
    stand_duration: int
    walk_duration: int
    sit_duration: int
    steps: int
    calories: int
    distance_cm: int


@dataclass(frozen=True)
class OsaDesaturation:
    od_time: int
    od_value: int
    od_duration: int


@dataclass(frozen=True)
class OsaResult:
    """Type 74: OSA result (ODI). timestamp is belongTime."""

    timestamp: int
    odi_valid_time: int
    odi_refined: int
    odi_score: int
    odi_disp_code: int
    version: int
    od_num_refined: int
    events: list


@dataclass(frozen=True)
class RestHrRealtime:
    """Type 86."""

    timestamp: int
    tz_quarters: int
    realtime_rest_hr: int
    rest_hr_baseline: int
    level: int


@dataclass(frozen=True)
class SleepRespiratoryEvent:
    """Type 57: opaque 569-byte event kept raw."""

    timestamp: int
    tz_quarters: int
    raw: bytes


@dataclass(frozen=True)
class RawHistoryRecord:
    """A record kept whole: sleep plan (66), device statistics (44), and every type without a parser."""

    code: int
    timestamp: int
    raw: bytes


@dataclass(frozen=True)
class PaiRecord:
    """Type 13 / PAI module 0x0022. Optional fields depend on version."""

    timestamp: int
    version: int
    timezone_raw: int
    activity_scores: list
    low_zone_pai: float
    medium_zone_pai: float
    high_zone_pai: float
    low_zone_minutes: int
    medium_zone_minutes: int
    high_zone_minutes: int
    daily_pai: float
    total_pai: float
    rest_hr: int
    max_hr: int
    index: int
    type: int
    next_activity_scores: Optional[list]
    gender: Optional[int]
    age: Optional[int]
    low_zone_limit: Optional[int]
    medium_zone_limit: Optional[int]
    high_zone_limit: Optional[int]


def _f32(r: ByteReader) -> float:
    return struct.unpack("<f", r.bytes(4))[0]


@_guarded
def parse_spot_hr(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 2: `ts u32, tz i8, hr u8`. Rejected if not a multiple of 6."""
    return _fixed(payload, 6, True, lambda r, i: SpotHeartRate(r.u32(), r.s8(), r.u8()))


@_guarded
def parse_today_activity_extended(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 88: 12-byte records per minute. Rejected if not a multiple of 12."""
    return _fixed(payload, 12, True, lambda r, i: TodayActivityExtended(
        ctx.header_time + i * 60, r.u8(), r.u8(), r.u8(), r.u8(), r.u8(), r.u8(), r.u8(), r.u8(), r.u8(), r.u8(),
        r.u16()), nxt=lambda recs: ctx.header_time + len(recs) * 60 + 1 if recs else None)


OSA_RESULT_SIZE = 16 + 240 * 12


@_guarded
def parse_osa_result(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 74: 2896-byte ODI result."""
    def read(r, i):
        belong, valid, refined = r.u32(), r.u32(), r.u32()
        score, disp, ver, num = r.u8(), r.u8(), r.u8(), r.u8()
        events = [OsaDesaturation(r.u32(), r.s32(), r.s32()) for _ in range(240)]
        return OsaResult(belong, valid, refined, score, disp, ver, num, events)
    return _fixed(payload, OSA_RESULT_SIZE, False, read)


@_guarded
def parse_rest_hr_realtime(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 86: `ts, tz, realtimeRestHr, restHrBaseline, level`."""
    return _fixed(payload, 8, False, lambda r, i: RestHrRealtime(r.u32(), r.s8(), r.u8(), r.u8(), r.u8()))


@_guarded
def parse_sleep_respiratory_event(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 57: opaque 569-byte events. The first five bytes are `ts, tz`."""
    def read(r, i):
        start = r.pos
        ts, tz = r.u32(), r.s8()
        r.pos = start
        return SleepRespiratoryEvent(ts, tz, r.bytes(569))
    return _fixed(payload, 569, False, read)


@_guarded
def parse_spo2_osa_event_alt(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 43: 5-byte `ts, descend`."""
    off = 1 if _is_spo2_versioned(payload, ctx) else 0
    return _fixed(payload, 5, False, lambda r, i: SpO2OsaEvent(r.u32(), r.u8(), None, None), offset=off)


SLEEP_PLAN_SIZE = 59
SLEEP_PLAN_PADDED = 64


@_guarded
def parse_sleep_plan(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 66: sleep plan structs, `uiTime u32` first, 59 bytes each, or 64 when the payload is a multiple of 64."""
    size = SLEEP_PLAN_PADDED if payload and len(payload) % SLEEP_PLAN_PADDED == 0 else SLEEP_PLAN_SIZE

    def read(r, i):
        start = r.pos
        t = r.u32()
        return RawHistoryRecord(66, t, payload[start:start + size])
    return _fixed(payload, size, False, read)


@_guarded
def parse_device_statistic(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 44: layout unknown. Kept whole under the chunk's header time."""
    if not payload:
        return ParseSuccess([], None)
    return ParseSuccess([RawHistoryRecord(44, ctx.header_time, payload)], None)


def parse_raw(code: int):
    """A parser that keeps the whole payload of a type without a parser."""
    @_guarded
    def parse(payload: bytes, ctx: ParseContext) -> ParseResult:
        if not payload:
            return ParseSuccess([], None)
        return ParseSuccess([RawHistoryRecord(code, ctx.header_time, payload)], None)
    return parse


@_guarded
def parse_pai(payload: bytes, ctx: ParseContext) -> ParseResult:
    """Type 13: the whole payload is one record."""
    if not payload:
        return ParseSuccess([], None)
    r = ByteReader(payload)
    v = r.u8()
    t, tz = r.u32(), r.u32()
    scores = [_f32(r) for _ in range(7)]
    low, med, high = _f32(r), _f32(r), _f32(r)
    low_m, med_m, high_m = r.u16(), r.u16(), r.u16()
    daily, total = _f32(r), _f32(r)
    rest_hr, max_hr, index, typ = r.u16(), r.u16(), r.u8(), r.u8()
    nxt = [_f32(r) for _ in range(7)] if v >= 2 else None
    gender = r.u8() if v >= 4 else None
    age = r.u8() if v >= 4 else None
    limits = (r.u8(), r.u8(), r.u8()) if v >= 5 else (None, None, None)
    rec = PaiRecord(t, v, tz, scores, low, med, high, low_m, med_m, high_m, daily, total, rest_hr, max_hr, index, typ,
                    nxt, gender, age, *limits)
    return ParseSuccess([rec], t + 1, r.remaining)


# ============================================================================================== activity layout


def _plausible_mode(b: int) -> bool:
    return b == 0xFF or b == ActivityMode.BLANK or (b & 0x0F) <= 9


def detect_activity_record_size(payload: bytes) -> int:
    if len(payload) % 8:
        return 4

    def score(size: int) -> float:
        n = len(payload) // size
        if n == 0:
            return 0.0
        return sum(1 for i in range(n) if _plausible_mode(payload[i * size])) / n

    return 8 if score(8) >= score(4) else 4


def activity_record_size(payload: bytes, size_field: int, feature_size: Optional[int] = None) -> int:
    """The START size field when it divides the payload into 4- or 8-byte records, else features, else detect."""
    unit = len(payload) // size_field if size_field > 0 and len(payload) % size_field == 0 else 0
    if unit in (4, 8):
        return unit
    return feature_size if feature_size is not None else detect_activity_record_size(payload)


# ============================================================================================== type specs

EXPECTED, IF_ADVERTISED = "expected", "if_advertised"


@dataclass(frozen=True)
class TypeSpec:
    code: int
    name: str
    sync_order: int
    parser: Callable
    strap: str
    loops: bool = True


SPECS: list[TypeSpec] = sorted([
    TypeSpec(1, "activity", 1, parse_activity, EXPECTED),
    TypeSpec(72, "sleep_result", 2, parse_sleep_result, EXPECTED),
    TypeSpec(73, "hrv_rmssd", 3, parse_hrv_rmssd, EXPECTED),
    TypeSpec(58, "resting_heart_rate", 4, parse_resting_hr, EXPECTED),
    TypeSpec(37, "spo2_single", 5, parse_spo2_single, EXPECTED),
    TypeSpec(38, "spo2_sleep", 5, parse_spo2_sleep, EXPECTED),
    TypeSpec(19, "stress_all_day", 6, parse_stress_all_day, EXPECTED),
    TypeSpec(18, "stress_single", 6, parse_stress_single, EXPECTED, loops=False),
    TypeSpec(56, "sleep_breath_rate", 7, parse_sleep_breath_rate, EXPECTED),
    TypeSpec(87, "sec_hr_v2", 8, parse_second_hr_v2, IF_ADVERTISED),
    TypeSpec(78, "hrv_sdnn", 10, parse_hrv_sdnn, IF_ADVERTISED),
    TypeSpec(46, "skin_temperature", 11, parse_skin_temperature, EXPECTED),
    TypeSpec(61, "daily_max_hr", 14, parse_daily_max_hr, IF_ADVERTISED),
    TypeSpec(39, "spo2_osa_event", 15, parse_spo2_osa_event, IF_ADVERTISED),
    TypeSpec(86, "rest_hr_realtime", 4, parse_rest_hr_realtime, IF_ADVERTISED),
    TypeSpec(76, "sec_hr", 8, parse_second_hr, IF_ADVERTISED),
    TypeSpec(88, "today_activity_extended", 9, parse_today_activity_extended, IF_ADVERTISED),
    TypeSpec(13, "pai", 11, parse_pai, EXPECTED),
    TypeSpec(74, "osa_result", 15, parse_osa_result, IF_ADVERTISED),
    TypeSpec(2, "spot_hr", 16, parse_spot_hr, IF_ADVERTISED),
    TypeSpec(57, "sleep_respiratory_event", 17, parse_sleep_respiratory_event, IF_ADVERTISED, loops=False),
    TypeSpec(43, "spo2_osa_event_alt", 18, parse_spo2_osa_event_alt, IF_ADVERTISED),
    TypeSpec(66, "sleep_plan", 20, parse_sleep_plan, IF_ADVERTISED),
    TypeSpec(44, "device_statistic", 21, parse_device_statistic, IF_ADVERTISED, loops=False),
], key=lambda s: s.sync_order)

SPECS_BY_CODE = {s.code: s for s in SPECS}
MAP_GATED = range(75, 97)


def plan(advertised: Optional[set[int]]) -> list[TypeSpec]:
    """Types 75..96 run only when the version reply lists them. Others: listed, or EXPECTED without a list."""
    out = []
    for s in SPECS:
        if s.code in MAP_GATED:
            if advertised and s.code in advertised:
                out.append(s)
        elif advertised is not None:
            if s.code in advertised:
                out.append(s)
        elif s.strap == EXPECTED:
            out.append(s)
    return out


def should_continue(spec: TypeSpec, current: int, nxt: Optional[int]) -> bool:
    return spec.loops and nxt is not None and nxt > current


# ============================================================================================== cursors (ZeppCursors)

CURSOR_FLOOR = 1_388_505_600
HOUR, DAY, MINUTE = 3600, 86400, 60


def day_start(epoch: int, zone: tzinfo) -> int:
    d = datetime.fromtimestamp(epoch, zone).date()
    return int(datetime(d.year, d.month, d.day, tzinfo=zone).timestamp())


def default_cursor(now: int, zone: tzinfo) -> int:
    """now - 2 days, floored to the start of its half day (Zepp `ty80.f`)."""
    t = datetime.fromtimestamp(now, zone) - timedelta(days=2)
    t = t.replace(hour=12 if t.hour >= 12 else 0, minute=0, second=0, microsecond=0)
    return int(t.timestamp())


def cursor_on_read(value: int, now: int) -> int:
    if value < CURSOR_FLOOR:
        return CURSOR_FLOOR
    if value > now + HOUR:
        return min(now - 2 * DAY, now)
    return value


def start_cursor(code: int, now: int, zone: tzinfo, read: Callable[[int], int]) -> int:
    """The first START time for a type, from the stored cursors (read gives one, or the default)."""
    if code == 72:
        return max(day_start(read(72), zone), now - 45 * DAY)
    if code == 58:
        return day_start(read(58), zone)
    if code == 61:
        return day_start(read(1), zone)
    if code in (37, 38, 39):
        return read(code) + 1
    if code in (76, 87):
        return read(76)
    if code in (74, 66):
        return read(code) - 1
    if code == 57:
        return read(56)
    if code in (5, 6):
        return read(5)
    if code == 59:
        v = read(59)
        if v > now:
            t = datetime.fromtimestamp(now, zone)
            y, m = (t.year, t.month - 1) if t.month > 1 else (t.year - 1, 12)
            t = t.replace(year=y, month=m, day=min(t.day, calendar.monthrange(y, m)[1]))
            t = t.replace(hour=12 if t.hour >= 12 else 0, minute=0, second=0, microsecond=0)
            return int(t.timestamp())
        return v
    return read(code)


def next_cursor(code: int, header_time: int, records: list, parsed: Optional[int]) -> Optional[int]:
    """The next START after a chunk, or None to stop."""
    if not records:
        return None
    last = max(r.timestamp for r in records)
    if code == 1:
        return header_time + len(records) * MINUTE
    if code in (88, 19):
        return header_time + len(records) * MINUTE + 1
    if code in (72, 73, 78, 74):
        return last + MINUTE
    if code in (37, 38, 39):
        return last + 30
    return parsed


def saved_cursor(code: int, records: list, nxt: Optional[int]) -> Optional[int]:
    """What to store for a type once its records are written, or None to leave it."""
    if not records:
        return None
    last = max(r.timestamp for r in records)
    if code == 1:
        return last + MINUTE
    if code in (73, 78, 70):
        return last + 1
    if code in (88, 72, 58, 74, 76, 87, 37, 38, 39, 59, 66):
        return last
    if code in (61, 57, 44):
        return None
    return nxt if nxt is not None else last + 1


def activity_kept(records: list, now: int) -> list:
    """Activity minutes up to the first one that starts after now and is unwritten (all 0xFF), as Zepp's `fnk`."""
    for i, r in enumerate(records):
        if r.timestamp > now and isinstance(r, ActivitySample) and r.is_blank:
            return records[:i]
    return records


RETRIES = 3


@dataclass
class TypeResult:
    spec: TypeSpec
    records: list = field(default_factory=list)
    next: Optional[int] = None
    ok: bool = True
    fast_recharge: bool = False


async def sync_type(client: GdspClient, spec: TypeSpec, start: int, zone: tzinfo,
                    log: Callable[[str], None] = lambda s: None, max_chunks: int = 2000) -> TypeResult:
    """Fetch one type from start, chunk after chunk, as the app's syncType does."""
    res = TypeResult(spec)
    cursor = start
    retries = RETRIES
    previous: Optional[PreviousChunk] = None
    for _ in range(max_chunks):
        f = await client.fetch(spec.code, cursor, zone, previous)
        if isinstance(f, FetchEmpty):
            break
        if isinstance(f, FetchFailed):
            log(f"history {spec.name}: {f.reason}")
            if f.fast_recharge:
                res.fast_recharge = True
            retries -= 1
            if f.cyclic or retries <= 0 or res.fast_recharge:
                res.ok = False
                break
            continue
        unit = f.unit
        if unit is None:
            log(f"history {spec.name}: got {len(f.payload)} bytes for size {f.size_field}, not read")
            res.ok = False
            break
        size = activity_record_size(f.payload, f.size_field) if spec.code == 1 else 4
        result = spec.parser(f.payload, ParseContext(f.header_time, activity_record_size=size))
        if isinstance(result, ParseFailure):
            log(f"history {spec.name}: parse failed: {result.reason}")
            res.ok = False
            break
        log(f"history {spec.name}: {len(f.payload)} bytes, {len(result.records)} records")
        res.records += result.records
        following = next_cursor(spec.code, f.header_time, result.records, result.next_cursor)
        if following is not None:
            res.next = following
        if not should_continue(spec, cursor, following):
            break
        previous = PreviousChunk(cursor, f.header_time)
        cursor = following  # type: ignore[assignment]
    return res


# ============================================================================================== JSON


_PROPS = {
    SpotHeartRate: ("heart_rate",),
    ActivitySample: ("heart_rate", "mode", "is_worn", "is_blank"),
    SecondHeartRate: ("heart_rate",),
    SleepResult: ("total_sleep_minutes", "wake_count"),
    SleepStage: ("type",),
    SpO2Reading: ("is_plausible",),
    StressSample: ("is_likely_gap",),
    SkinTemperatureSample: ("skin_celsius",),
    HrvSdnn: ("sdnn_ms",),
}


def to_json(obj):
    """A record as plain JSON-ready data, with its derived fields."""
    if dataclasses.is_dataclass(obj) and not isinstance(obj, type):
        d = {f.name: to_json(getattr(obj, f.name)) for f in dataclasses.fields(obj)}
        for p in _PROPS.get(type(obj), ()):
            d[p] = getattr(obj, p)
        return d
    if isinstance(obj, (list, tuple)):
        return [to_json(x) for x in obj]
    if isinstance(obj, (bytes, bytearray)):
        return obj.hex()
    return obj
