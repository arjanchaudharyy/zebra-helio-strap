"""Workout history: GDSP 5 (summary, protobuf), 6 (detail, TLV), 59 (sport effect), 70 (recovery HR).

Port of history/Workouts.kt and HistoryParsers.recoveryHr. The protobuf reader is a minimal hand-written one.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass
from datetime import tzinfo
from typing import Optional, Union

from ._bytes import ByteReader, ShortRead, signed
from .history import (FetchChunk, FetchFailed, GdspClient, ParseContext, ParseFailure, ParseSuccess,
                      _guarded, _last_plus_one, valid_hr)

# ============================================================================================== protobuf


class Proto:
    """Only the scalar fields used. Other fields are skipped by wire type. Raises ValueError on bad input."""

    def __init__(self, b: bytes, i: int = 0, end: Optional[int] = None) -> None:
        self.b = b
        self.i = i
        self.end = len(b) if end is None else end
        self.ints: dict[int, int] = {}
        self.floats: dict[int, float] = {}
        self.messages: dict[int, "Proto"] = {}
        while self.i < self.end:
            key = self._varint()
            fld = key >> 3
            wt = key & 7
            if wt == 0:
                self.ints[fld] = self._varint()
            elif wt == 1:
                self.i += 8
            elif wt == 2:
                n = self._varint()
                if n < 0 or self.i + n > self.end:
                    raise ValueError(f"field {fld} overruns")
                try:  # strings and packed fields are not messages: skip them
                    self.messages[fld] = Proto(b, self.i, self.i + n)
                except (ValueError, IndexError):
                    pass
                self.i += n
            elif wt == 5:
                if self.i + 4 > len(b):
                    raise ShortRead("fixed32 past end")
                self.floats[fld] = struct.unpack_from("<f", b, self.i)[0]
                self.i += 4
            else:
                raise ValueError(f"wire type {wt} at {self.i - 1}")
        if self.i != self.end:
            raise ValueError("message overruns")

    def _varint(self) -> int:
        r = 0
        shift = 0
        while True:
            if self.i >= self.end:
                raise ValueError("truncated varint")
            c = self.b[self.i]
            self.i += 1
            r |= (c & 0x7F) << shift
            if c < 0x80:
                return r & 0xFFFFFFFFFFFFFFFF
            shift += 7
            if shift >= 64:
                raise ValueError("varint too long")

    def int(self, fld: int) -> Optional[int]:
        """The field as a Kotlin Int (low 32 bits, signed)."""
        v = self.ints.get(fld)
        return None if v is None else signed(v, 32)


def _f32(v: Optional[float]) -> Optional[float]:
    return None if v is None else float(v)


# ============================================================================================== records


@dataclass(frozen=True)
class WorkoutSummary:
    """Type 5: one workout. timestamp is the trackId (the workout start epoch). Seconds."""

    timestamp: int
    sport_type: int
    subtype: Optional[int]
    total_time: Optional[int]
    exercise_time: Optional[int]
    calories: Optional[int]
    average_heart_rate: Optional[int]
    max_heart_rate: Optional[int]
    min_heart_rate: Optional[int]
    aerobic_training_effect: Optional[float]
    anaerobic_training_effect: Optional[float]
    training_load: Optional[int]
    vo2max: Optional[int]
    steps: Optional[int]

    @property
    def track_id(self) -> int:
        return self.timestamp


@dataclass(frozen=True)
class WorkoutDetail:
    """Type 6: heart_rate is a list of (epoch ms, bpm). events is (epoch ms, code)."""

    timestamp: int
    heart_rate: list
    events: list

    RECOVERY_TAIL_S = 180


@dataclass(frozen=True)
class SportEffect:
    """Type 59: the strap's result after a workout. timestamp is the workout end."""

    timestamp: int
    maximal_heart_rate: Optional[int]
    epoc: Optional[float]
    vo2max: Optional[float]
    aerobic_training_effect: Optional[float]
    anaerobic_training_effect: Optional[float]
    recovery_time_minutes: Optional[int]
    training_load_weekly: Optional[float]
    training_load_monthly: Optional[float]
    optimal_load_min: Optional[int]
    optimal_load_max: Optional[int]
    last_valid_vo2max: Optional[int]


@dataclass(frozen=True)
class RecoveryHeartRate:
    """Type 70: recovery HR after a workout. timestamp is the trackId."""

    timestamp: int
    samples: list


def _opt_hr(v: Optional[int]) -> Optional[int]:
    return None if v is None else valid_hr(v)


# ============================================================================================== parsers

PROTOBUF_SUMMARY = 0x8000


@_guarded
def parse_summary(payload: bytes, ctx: ParseContext):
    """Type 5: `u16 version` (0x8000 = protobuf), then SportSummaryMessage. One workout per START."""
    if len(payload) <= 2:
        return ParseSuccess([], None)
    version = payload[0] | (payload[1] << 8)
    if version != PROTOBUF_SUMMARY:
        return ParseFailure(f"summary version {version} not supported")
    m = Proto(payload, 2)
    typ = m.messages.get(3)
    if typ is None or typ.int(1) is None:
        return ParseFailure("summary without sport type")
    record, times, hr, other = m.messages.get(2), m.messages.get(7), m.messages.get(19), m.messages.get(21)
    rec_start = record.ints.get(1) if record else None
    start = rec_start if rec_start and rec_start > 0 else ctx.header_time
    vo2 = other.int(7) if other else None
    w = WorkoutSummary(
        timestamp=start,
        sport_type=typ.int(1),
        subtype=typ.int(2),
        total_time=times.int(1) if times else None,
        exercise_time=times.int(2) if times else None,
        calories=m.messages[16].int(1) if 16 in m.messages else None,
        average_heart_rate=_opt_hr(hr.int(1)) if hr else None,
        max_heart_rate=_opt_hr(hr.int(2)) if hr else None,
        min_heart_rate=_opt_hr(hr.int(3)) if hr else None,
        aerobic_training_effect=_f32(other.floats.get(4)) if other else None,
        anaerobic_training_effect=_f32(other.floats.get(5)) if other else None,
        training_load=other.int(6) if other else None,
        vo2max=vo2 if vo2 is not None and vo2 > 0 else None,
        steps=m.messages[11].int(4) if 11 in m.messages else None,
    )
    return ParseSuccess([w], start + 1)


TAG_TIMESTAMP = 1
TAG_EVENT = 4
TAG_HEART_RATE = 8


@_guarded
def parse_detail(payload: bytes, ctx: ParseContext):
    """Type 6: TLV nodes. TimeStamp (4 bytes, then u64 epoch ms) sets the base. HeartRate nodes are
    `offset u16 (ms), bpm u8`. Event nodes are `offset u16, code u16`. Other nodes are skipped."""
    r = ByteReader(payload)
    base: Optional[int] = None
    hr: list = []
    events: list = []
    while r.remaining > 0:
        b0 = r.u8()
        tag = b0 & 0x1F
        if tag == 0x1F:
            tag = b0
            k = 1
            while True:
                c = r.u8()
                tag |= (c & 0x7F) << (8 * k)
                k += 1
                if c < 0x80:
                    break
        l0 = r.u8()
        if l0 & 0x80 == 0:
            n = l0
        else:
            n = 0
            for _ in range(l0 & 0x7F):
                n = (n << 8) | r.u8()
        if n > r.remaining:
            raise ValueError(f"node {tag} overruns")
        v = ByteReader(r.bytes(n))
        if tag == TAG_TIMESTAMP and n >= 12:
            v.bytes(4)
            base = v.u32() | (v.u32() << 32)
        elif tag == TAG_HEART_RATE and n >= 3 and base is not None:
            off = v.u16()
            bpm = valid_hr(v.u8())
            if bpm is not None:
                hr.append((base + off, bpm))
        elif tag == TAG_EVENT and n >= 4 and base is not None:
            off = v.u16()
            events.append((base + off, v.u16()))
    if not hr and not events:
        return ParseSuccess([], ctx.header_time + 1)
    return ParseSuccess([WorkoutDetail(ctx.header_time, hr, events)], ctx.header_time + 1)


@_guarded
def parse_sport_effect(payload: bytes, ctx: ParseContext):
    """Type 59: a bare SportEffectMessage. One per workout."""
    if not payload:
        return ParseSuccess([], None)
    m = Proto(payload)
    end_raw = m.ints.get(26)
    end = end_raw if end_raw and end_raw > 0 else ctx.header_time
    vo2 = m.floats.get(7)
    last_vo2 = m.int(21)
    e = SportEffect(
        timestamp=end,
        maximal_heart_rate=_opt_hr(m.int(4)),
        epoc=_f32(m.floats.get(6)),
        vo2max=vo2 if vo2 is not None and vo2 > 0 else None,
        aerobic_training_effect=_f32(m.floats.get(8)),
        anaerobic_training_effect=_f32(m.floats.get(9)),
        recovery_time_minutes=m.int(11),
        training_load_weekly=_f32(m.floats.get(15)),
        training_load_monthly=_f32(m.floats.get(16)),
        optimal_load_min=m.int(12),
        optimal_load_max=m.int(13),
        last_valid_vo2max=last_vo2 if last_vo2 is not None and last_vo2 > 0 else None,
    )
    return ParseSuccess([e], max(end, ctx.header_time) + 1)


@_guarded
def parse_recovery_hr(payload: bytes, ctx: ParseContext):
    """Type 70: blocks `trackId u32, count u32, hr[count]`. Stops at the first truncated block."""
    r = ByteReader(payload)
    out = []
    while r.remaining >= 8:
        start = r.pos
        track, count = r.u32(), r.u32()
        if count > r.remaining:
            r.pos = start
            break
        out.append(RecoveryHeartRate(track, list(r.bytes(count))))
    return ParseSuccess(out, _last_plus_one(out), r.remaining)


# ============================================================================================== fetch


@dataclass
class WorkoutNone:
    pass


@dataclass
class WorkoutFailed:
    reason: str
    fast_recharge: bool = False


@dataclass
class WorkoutUnreadable:
    reason: str
    next_cursor: int


@dataclass
class Workout:
    summary: WorkoutSummary
    detail: Optional[WorkoutDetail]
    next_cursor: int
    retry_detail: bool = False


WorkoutFetch = Union[WorkoutNone, WorkoutFailed, WorkoutUnreadable, Workout]


async def fetch_workout(client: GdspClient, since: int, zone: tzinfo) -> WorkoutFetch:
    """One workout as Zepp's `pxk` fetches it: START(5, cursor), then START(6, same cursor). No retries."""
    s = await client.fetch(5, since, zone)
    if isinstance(s, FetchFailed):
        return WorkoutFailed(f"summary: {s.reason}", s.fast_recharge)
    if not isinstance(s, FetchChunk):
        return WorkoutNone()
    parsed = parse_summary(s.payload, ParseContext(s.header_time))
    # The detail is asked for whether or not the summary parses, as Zepp does.
    d = await client.fetch(6, since, zone)
    nxt = s.header_time + 1
    if isinstance(parsed, ParseFailure):
        return WorkoutUnreadable(f"summary: {parsed.reason}", nxt)
    if not parsed.records:
        return WorkoutNone()
    if len(parsed.records) != 1:
        return WorkoutUnreadable(f"summary: {len(parsed.records)} workouts in one", nxt)
    summary = parsed.records[0]
    retry = False
    detail = None
    if isinstance(d, FetchChunk):
        r = parse_detail(d.payload, ParseContext(summary.track_id))
        if isinstance(r, ParseSuccess):
            detail = r.records[0] if len(r.records) == 1 else None
        else:
            client.log(f"history 6: {r.reason}")
    elif isinstance(d, FetchFailed):
        client.log(f"history 6: {d.reason}")
        retry = True
    return Workout(summary, detail, nxt, retry)


@dataclass
class WorkoutsResult:
    workouts: list
    ok: bool
    fast_recharge: bool = False
    saved_cursor: Optional[int] = None


async def sync_workouts(client: GdspClient, start: int, zone: tzinfo, now: int,
                        max_workouts: int = 500) -> WorkoutsResult:
    """The app's syncWorkouts loop: one workout per round until None or a failure."""
    cursor = start
    pairs: list[Workout] = []
    unreadable = 0
    for _ in range(max_workouts):
        w = await fetch_workout(client, cursor, zone)
        if isinstance(w, WorkoutNone):
            break
        if isinstance(w, WorkoutFailed):
            client.log(f"history workout: {w.reason}")
            return WorkoutsResult(pairs, False, w.fast_recharge)
        if isinstance(w, WorkoutUnreadable):
            client.log(f"history workout: {w.reason}, left out")
            unreadable += 1
        else:
            if w.retry_detail:
                client.log("history workout: detail failed, nothing stored")
                return WorkoutsResult([], False)
            pairs.append(w)
        if w.next_cursor <= cursor:
            break
        cursor = w.next_cursor
    saved = None
    if pairs:
        last = max(p.summary.track_id for p in pairs) + 1
        saved = now if last > now else last
    return WorkoutsResult(pairs, unreadable == 0, saved_cursor=saved)
