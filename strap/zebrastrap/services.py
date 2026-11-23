"""Channel modules for PAI (0x0022), readiness (0x0049) and sleep (0x0032) (ZeppChannelJobs.kt, Readiness.kt).

All requests here are read-only queries.
"""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass
from typing import Optional

from ._bytes import ByteReader, ByteWriter, u32
from .history import PaiRecord, ParseContext, ParseSuccess, parse_pai


class PaiCodec:
    """PaiModule 0x0022: Zepp reads PAI here when the module version is 5 or more."""

    QUERY = 0x0C
    REPLY = 0x0D
    RECORD = 102
    HEADER = 3
    FULL_RECORD_VERSION = 5
    version_request = b"\x00"

    @staticmethod
    def parse_version(reply: bytes) -> Optional[int]:
        """`01 xx version`: the version is byte 2."""
        return reply[2] if len(reply) > 2 and reply[0] == 0x01 else None

    @staticmethod
    def query(since: int) -> bytes:
        return ByteWriter().u8(PaiCodec.QUERY).u32(since).to_bytes()

    @staticmethod
    def is_reply(reply: bytes) -> bool:
        return len(reply) > 0 and reply[0] == PaiCodec.REPLY

    @staticmethod
    def parse(reply: bytes) -> Optional[list[PaiRecord]]:
        """`0D status count` then 102-byte records, read whole whatever their version byte says.
        Any status but 0 is an empty result. None only when it is not a reply."""
        if not PaiCodec.is_reply(reply):
            return None
        if len(reply) < PaiCodec.HEADER or reply[1] != 0:
            return []
        out = []
        for at in range(PaiCodec.HEADER, len(reply) - PaiCodec.RECORD + 1, PaiCodec.RECORD):
            record = bytearray(reply[at:at + PaiCodec.RECORD])
            version = record[0]
            record[0] = PaiCodec.FULL_RECORD_VERSION
            parsed = parse_pai(bytes(record), ParseContext(0))
            if isinstance(parsed, ParseSuccess) and len(parsed.records) == 1:
                out.append(dataclasses.replace(parsed.records[0], version=version))
        return out


class SleepModuleCodec:
    """Sleep module 0x0032: `01` once per strap. The sleep plan job (66) runs when the version is 2 or more."""

    version_request = b"\x01"
    SLEEP_PLAN_VERSION = 2

    @staticmethod
    def parse_version(reply: bytes) -> Optional[int]:
        return reply[1] if len(reply) >= 2 and reply[0] == 0x02 else None


@dataclass(frozen=True)
class StrapReadiness:
    """The strap's morning readiness. day is local midnight of the day it scores. None = unset (0xFF)."""

    day: int
    updated_at: int
    tz_quarters: int
    status: int
    score: Optional[int]
    hrv_score: Optional[int]
    resting_hr_score: Optional[int]
    skin_temperature_score: Optional[int]
    breathing_score: Optional[int]
    physical_score: Optional[int]
    mental_score: Optional[int]
    sleep_hrv: Optional[int]
    sleep_resting_hr: Optional[int]
    algorithm_version: str
    insight_id: int

    @property
    def timestamp(self) -> int:
        return self.day


@dataclass(frozen=True)
class ReadinessRaw:
    """A readiness module record kept whole (ops 06 and 0C)."""

    op: int
    timestamp: int
    raw: bytes


def _pct(v: int) -> Optional[int]:
    return v if 0 <= v <= 100 else None


class ReadinessCodec:
    """`00` version, `08 <since u32>` score. Replies are `<cmd+1> status ...`, status 0 = OK, 2 = no data.
    Commands 02, 04 and 0E write data to the strap and are not used."""

    VERSION = 0x00
    SCORE = 0x08
    DATA = 0x06
    STRESS_THRESHOLD = 0x0C
    SCORE_RECORD = 44
    RECORDS_AT = 3
    NO_DATA = 2
    version_request = b"\x00"

    @staticmethod
    def score_request(since: int) -> bytes:
        return ByteWriter().u8(ReadinessCodec.SCORE).u32(since).to_bytes()

    @staticmethod
    def request(op: int, since: int) -> bytes:
        return ByteWriter().u8(op).u32(since).to_bytes()

    @staticmethod
    def parse_version(reply: bytes) -> Optional[int]:
        return reply[2] if len(reply) >= 3 and reply[0] == ReadinessCodec.VERSION + 1 else None

    @staticmethod
    def parse_records(op: int, reply: bytes) -> Optional[list[ReadinessRaw]]:
        """`<op+1> status count` then count equal-size records, each starting with its u32 time."""
        status = reply[1] if len(reply) >= 3 and reply[0] == op + 1 else None
        count = reply[2] if status == 0 else 0
        size = (len(reply) - 3) // count if count > 0 else 0
        if status == ReadinessCodec.NO_DATA or (status == 0 and count == 0):
            return []
        if status != 0 or size < 4:
            return None
        out = []
        for i in range(count):
            rec = reply[3 + i * size:3 + (i + 1) * size]
            out.append(ReadinessRaw(op, u32(rec, 0), bytes(rec)))
        return out

    @staticmethod
    def parse_scores(reply: bytes) -> Optional[list[StrapReadiness]]:
        """`09 status n` then n 44-byte records. None if not a score reply."""
        if len(reply) < 3 or reply[0] != ReadinessCodec.SCORE + 1:
            return None
        if reply[1] != 0:
            return []
        r = ByteReader(reply, 3)
        out = []
        for _ in range(reply[2]):
            if r.remaining < ReadinessCodec.SCORE_RECORD:
                return out
            ts, updated, tz = r.u32(), r.u32(), r.s8()
            status, hrv, rhr, insight = r.u8(), r.u8(), r.u8(), r.u8()
            alg, alg_sub = r.u8(), r.u8()
            score, phy, ment, hrv_s, skin_s, rhr_s, ahi_s = (r.u8() for _ in range(7))
            r.bytes(ReadinessCodec.SCORE_RECORD - 22)
            out.append(StrapReadiness(
                day=ts, updated_at=updated, tz_quarters=tz, status=status, score=_pct(score), hrv_score=_pct(hrv_s),
                resting_hr_score=_pct(rhr_s), skin_temperature_score=_pct(skin_s), breathing_score=_pct(ahi_s),
                physical_score=_pct(phy), mental_score=_pct(ment),
                sleep_hrv=hrv if 1 <= hrv <= 254 else None, sleep_resting_hr=rhr if 25 <= rhr <= 220 else None,
                algorithm_version=f"{alg}.{alg_sub}", insight_id=insight))
        return out
