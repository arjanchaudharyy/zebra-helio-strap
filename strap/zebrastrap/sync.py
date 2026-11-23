"""History sync runner: Zepp 10.10.5's job list and order (app ZeppJobs.kt), JSON lines output, re-sync dedupe.

Default job list on the Helio Strap: 58, 66, 72, 1, readiness 08, 5->6, 0C, 06, 73, 19, 18, 37, 38, 39, 59, PAI,
56, 57, 70, 61, 74, 76, 87, 88, 78, 44 (once a day), then 46. After that, every other type that the GDSP version
reply lists is fetched raw (bytes as hex), unless zepp_only is set.
"""

from __future__ import annotations

import json
import time
from dataclasses import dataclass
from datetime import tzinfo
from pathlib import Path
from typing import Callable, Optional

from . import history as h
from . import workouts as wk
from .auth import Endpoints
from .services import PaiCodec, ReadinessCodec, SleepModuleCodec

CHANNEL_JOB_TIMEOUT = 10.0

EXTRA_SPECS = {
    59: h.TypeSpec(59, "sport_effect", 12, wk.parse_sport_effect, h.IF_ADVERTISED, loops=False),
    70: h.TypeSpec(70, "recovery_hr", 13, wk.parse_recovery_hr, h.IF_ADVERTISED),
}


def spec_for(code: int) -> Optional[h.TypeSpec]:
    return h.SPECS_BY_CODE.get(code) or EXTRA_SPECS.get(code)


@dataclass(frozen=True)
class Job:
    kind: str  # "gdsp", "workouts", "readiness", "pai", "raw"
    code: int  # GDSP type code, or the readiness op

    @property
    def label(self) -> str:
        if self.kind == "readiness":
            return f"r{self.code:02x}"
        if self.kind == "pai":
            return "pai"
        if self.kind == "raw":
            return f"raw{self.code}"
        return str(self.code)


def zepp_jobs(advertised: Optional[set[int]], has_endpoint: Callable[[int], bool], readiness_version: Optional[int],
              pai_version: Optional[int], sleep_plan: bool, statistics_due: bool) -> list[Job]:
    """ZeppJobs.list without the 0x0028 feature bitmap: each feature gate falls back to the advertised type map."""
    adv = advertised or set()
    jobs: list[Job] = []

    def bit(code: int) -> bool:
        return code in adv

    def gdsp(code: int) -> None:
        if code == 5:
            jobs.append(Job("workouts", 5))
        elif spec_for(code) is not None:
            jobs.append(Job("gdsp", code))

    readiness = has_endpoint(Endpoints.READINESS)
    if bit(58):
        gdsp(58)
    if sleep_plan:
        gdsp(66)
    if bit(72):
        gdsp(72)
    gdsp(1)
    if readiness:
        jobs.append(Job("readiness", ReadinessCodec.SCORE))
    gdsp(5)
    if readiness and (readiness_version or 0) >= 3:
        jobs.append(Job("readiness", ReadinessCodec.STRESS_THRESHOLD))
    if readiness:
        jobs.append(Job("readiness", ReadinessCodec.DATA))
    if bit(73):
        gdsp(73)
    if bit(19):
        gdsp(19)
        gdsp(18)
    if bit(37):
        gdsp(37)
        if bit(38):
            gdsp(38)
            gdsp(39)
    if bit(59):  # bit 10 (FirstBeat) is unknown without the bitmap, so it counts as clear
        gdsp(59)
    if (pai_version or 0) >= 5 and has_endpoint(Endpoints.PAI):
        jobs.append(Job("pai", 13))
    else:
        gdsp(13)
    gdsp(56)
    gdsp(57)
    if bit(70):
        gdsp(70)
    if bit(61):
        gdsp(61)
    if bit(74):
        gdsp(74)
    for code in (76, 87, 88, 78):
        if bit(code):
            gdsp(code)
    if bit(44) and statistics_due:
        gdsp(44)
    if bit(46):
        gdsp(46)
    return jobs


def raw_jobs(advertised: Optional[set[int]], jobs: list[Job]) -> list[Job]:
    """Every advertised type that no job above covers. 6 goes with 5, so it is covered."""
    covered = {j.code for j in jobs if j.kind in ("gdsp", "workouts")} | {6}
    if any(j.kind == "pai" for j in jobs):
        covered.add(13)
    return [Job("raw", c) for c in sorted(advertised or ()) if c not in covered]


def select(jobs: list[Job], wanted: Optional[set[str]]) -> list[Job]:
    """--types filter. Entries are GDSP codes ("72"), "pai", "readiness" or "r08"/"r0c"/"r06"."""
    if not wanted:
        return jobs
    out = []
    for j in jobs:
        keys = {j.label, str(j.code)} if j.kind != "readiness" else {j.label, "readiness"}
        if j.kind == "pai":
            keys |= {"13"}
        if j.kind == "workouts":
            keys |= {"6"}
        if keys & wanted:
            out.append(j)
    return out


# ============================================================================================== output


def _key(rec) -> str:
    raw = getattr(rec, "raw", None)
    base = str(getattr(rec, "timestamp", ""))
    if isinstance(raw, (bytes, bytearray)):
        return f"{base}:{raw.hex()}"
    return base


class JsonlStore:
    """Appends records as JSON lines, one file per type. A record whose key (timestamp, plus the bytes for raw
    records) is already in the file is skipped, so a re-sync does not repeat lines."""

    def __init__(self, out: Path) -> None:
        self.out = out
        self._keys: dict[str, set[str]] = {}

    def _existing(self, name: str) -> set[str]:
        if name not in self._keys:
            keys = set()
            path = self.out / name
            if path.exists():
                for line in path.read_text().splitlines():
                    try:
                        keys.add(json.loads(line).get("_key", ""))
                    except ValueError:
                        pass
            self._keys[name] = keys
        return self._keys[name]

    def write(self, name: str, type_label, records: list) -> int:
        seen = self._existing(name)
        n = 0
        with (self.out / name).open("a") as f:
            for rec in records:
                k = _key(rec)
                if k in seen:
                    continue
                seen.add(k)
                f.write(json.dumps({"type": type_label, "_key": k, **h.to_json(rec)}) + "\n")
                n += 1
        return n


# ============================================================================================== runner


@dataclass
class JobReport:
    job: Job
    fetched: int
    written: int
    ok: bool
    file: Optional[str] = None


class SyncRunner:
    def __init__(self, session, gdsp: h.GdspClient, out: Path, zone: tzinfo, log: Callable[[str], None] = print,
                 days: Optional[float] = None, now: Optional[int] = None) -> None:
        self.session = session
        self.gdsp = gdsp
        self.out = out
        self.zone = zone
        self.log = log
        self.days = days
        self.now = now if now is not None else int(time.time())
        self.cursor_file = out / "cursors.json"
        try:
            self.cursors: dict[str, int] = {str(k): int(v) for k, v in json.loads(self.cursor_file.read_text()).items()}
        except (OSError, ValueError):
            self.cursors = {}
        self.store = JsonlStore(out)
        self.fast_recharge = False

    # ---- cursors
    def _save(self, key: str, value: int) -> None:
        if value < h.CURSOR_FLOOR:
            value = h.CURSOR_FLOOR
        elif value > self.now + h.HOUR:
            value = self.now
        self.cursors[key] = value
        self.cursor_file.write_text(json.dumps(self.cursors, indent=1, sort_keys=True))

    def read(self, key) -> int:
        if self.days is not None:
            return self.now - int(self.days * 86400)
        stored = self.cursors.get(str(key))
        return h.cursor_on_read(stored, self.now) if stored is not None else h.default_cursor(self.now, self.zone)

    def _start(self, code: int) -> int:
        return h.start_cursor(code, self.now, self.zone, lambda c: self.read(c))

    # ---- module queries
    async def _module_version(self, endpoint: int, request: bytes, parse) -> Optional[int]:
        if not self.session.services.has(endpoint):
            return None
        try:
            reply = await self.session.request(endpoint, request, timeout=CHANNEL_JOB_TIMEOUT,
                                               matches=lambda b: parse(b) is not None)
        except Exception as e:
            self.log(f"module {endpoint:04x} version: {e}")
            return None
        return parse(reply)

    async def plan(self, zepp_only: bool = False) -> tuple[list[Job], Optional[set[int]]]:
        version = await self.gdsp.query_version()
        version = version if version is not None and version.is_success else None
        advertised = version.supported_types() if version else None
        self.log(f"gdsp version {version.version if version else None}, "
                 f"strap offers {sorted(advertised) if advertised else advertised}")
        sleep_v = self.cursors.get("sleep_module")
        if sleep_v is None:
            sleep_v = await self._module_version(Endpoints.SLEEP, SleepModuleCodec.version_request,
                                                 SleepModuleCodec.parse_version)
            if sleep_v is not None:
                self._store_plain("sleep_module", sleep_v)
        pai_v = await self._module_version(Endpoints.PAI, PaiCodec.version_request, PaiCodec.parse_version)
        readiness_v = await self._module_version(Endpoints.READINESS, ReadinessCodec.version_request,
                                                 ReadinessCodec.parse_version)
        last_stats = self.cursors.get("44")
        stats_due = last_stats is None or h.day_start(last_stats, self.zone) != h.day_start(self.now, self.zone)
        jobs = zepp_jobs(advertised, self.session.services.has, readiness_v, pai_v,
                         (sleep_v or 0) >= SleepModuleCodec.SLEEP_PLAN_VERSION, stats_due)
        if not zepp_only:
            jobs += raw_jobs(advertised, jobs)
        return jobs, advertised

    def _store_plain(self, key: str, value: int) -> None:
        self.cursors[key] = value
        self.cursor_file.write_text(json.dumps(self.cursors, indent=1, sort_keys=True))

    # ---- jobs
    async def run(self, jobs: list[Job]) -> list[JobReport]:
        reports = []
        for job in jobs:
            if self.fast_recharge:
                self.log("strap is in fast-recharge mode: sync stopped")
                break
            try:
                if job.kind == "gdsp":
                    r = await self._gdsp(job)
                elif job.kind == "workouts":
                    r = await self._workouts(job)
                elif job.kind == "readiness":
                    r = await self._readiness(job)
                elif job.kind == "pai":
                    r = await self._pai(job)
                else:
                    r = await self._raw(job)
            except Exception as e:
                self.log(f"job {job.label}: {type(e).__name__}: {e}")
                r = JobReport(job, 0, 0, False)
            if job.code == 57 and job.kind == "gdsp":
                r.ok = True  # Zepp ignores the breath events' result
            reports.append(r)
        return reports

    async def _gdsp(self, job: Job) -> JobReport:
        spec = spec_for(job.code)
        res = await h.sync_type(self.gdsp, spec, self._start(job.code), self.zone, self.log)
        self.fast_recharge |= res.fast_recharge
        records = res.records
        if job.code == 1:
            records = h.activity_kept(records, int(time.time()))
        stored = [r for r in records if not (isinstance(r, h.ActivitySample) and r.is_blank)]
        name = f"{job.code:02d}_{spec.name}.jsonl"
        written = self.store.write(name, job.code, stored) if stored else 0
        if job.code == 44:
            if res.ok:
                self._save("44", self.now)
        else:
            saved = h.saved_cursor(job.code, records, res.next)
            if saved is not None:
                self._save(str(job.code), saved)
        return JobReport(job, len(stored), written, res.ok, name if stored else None)

    async def _workouts(self, job: Job) -> JobReport:
        res = await wk.sync_workouts(self.gdsp, self._start(5), self.zone, self.now)
        self.fast_recharge |= res.fast_recharge
        summaries = [w.summary for w in res.workouts]
        details = [w.detail for w in res.workouts if w.detail is not None]
        written = self.store.write("05_workout_summary.jsonl", 5, summaries) if summaries else 0
        if details:
            self.store.write("06_workout_detail.jsonl", 6, details)
        if res.saved_cursor is not None:
            self._save("5", res.saved_cursor)
        return JobReport(job, len(summaries), written, res.ok, "05_workout_summary.jsonl" if summaries else None)

    async def _readiness(self, job: Job) -> JobReport:
        op = job.code
        key = f"r{op:02x}"
        reply = await self.session.request(Endpoints.READINESS, ReadinessCodec.request(op, self.read(key)),
                                           timeout=CHANNEL_JOB_TIMEOUT, matches=lambda b: len(b) > 0 and b[0] == op + 1)
        records = ReadinessCodec.parse_scores(reply) if op == ReadinessCodec.SCORE else \
            ReadinessCodec.parse_records(op, reply)
        if records is None:
            self.log(f"readiness {op:02x}: unexpected reply {reply.hex()}")
            return JobReport(job, 0, 0, False)
        name = f"readiness_{op:02x}.jsonl"
        written = self.store.write(name, key, records) if records else 0
        last = max((r.timestamp for r in records), default=0)
        if last > 0:
            self._save(key, last + 1)
        return JobReport(job, len(records), written, True, name if records else None)

    async def _pai(self, job: Job) -> JobReport:
        reply = await self.session.request(Endpoints.PAI, PaiCodec.query(self.read("pai")),
                                           timeout=CHANNEL_JOB_TIMEOUT, matches=PaiCodec.is_reply)
        records = PaiCodec.parse(reply)
        if records is None:
            return JobReport(job, 0, 0, False)
        name = "13_pai.jsonl"
        written = self.store.write(name, 13, records) if records else 0
        if records:
            self._save("pai", max(r.timestamp for r in records) + 1)
        return JobReport(job, len(records), written, True, name if records else None)

    async def _raw(self, job: Job, max_chunks: int = 50) -> JobReport:
        """A type without a parser: keep each chunk's bytes. The next START is the chunk's header time + 1 s."""
        code = job.code
        cursor = self.read(code)
        records = []
        ok = True
        previous: Optional[h.PreviousChunk] = None
        for _ in range(max_chunks):
            f = await self.gdsp.fetch(code, cursor, self.zone, previous)
            if isinstance(f, h.FetchEmpty):
                break
            if isinstance(f, h.FetchFailed):
                self.log(f"raw {code}: {f.reason}")
                self.fast_recharge |= f.fast_recharge
                ok = False
                break
            records.append(h.RawHistoryRecord(code, f.header_time, f.payload))
            nxt = f.header_time + 1
            if nxt <= cursor:
                break
            previous = h.PreviousChunk(cursor, f.header_time)
            cursor = nxt
        name = f"{code:02d}_raw.jsonl"
        written = self.store.write(name, code, records) if records else 0
        if records:
            self._save(str(code), cursor)
        return JobReport(job, len(records), written, ok, name if records else None)
