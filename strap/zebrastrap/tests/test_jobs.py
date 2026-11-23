"""ZeppJobsTest.kt, without the 0x0028 feature bitmap (each gate falls back to the advertised type map)."""

from zebrastrap import sync
from zebrastrap.auth import Endpoints

# The Helio Strap's GDSP version reply type map (fw 3.27.0.1): 35 types.
ADVERTISED = {1, 2, 5, 6, 7, 8, 11, 13, 17, 18, 19, 30, 37, 38, 39, 44, 46, 48, 56, 57, 58, 59, 61, 63, 64, 66, 70,
              72, 73, 74, 76, 78, 85, 87, 88}
ALL_ENDPOINTS = {Endpoints.READINESS, Endpoints.PAI, Endpoints.SLEEP}


def jobs(endpoints=ALL_ENDPOINTS, readiness=3, pai=5, stats=True, advertised=ADVERTISED, sleep_plan=True):
    return sync.zepp_jobs(advertised, endpoints.__contains__, readiness, pai, sleep_plan, stats)


def names(js):
    return [j.label for j in js]


def test_the_straps_job_list_is_zepps_then_skin_temperature():
    assert len(ADVERTISED) == 35
    assert names(jobs()) == ["58", "66", "72", "1", "r08", "5", "r0c", "r06", "73", "19", "18", "37", "38", "39", "59",
                             "pai", "56", "57", "70", "61", "74", "76", "87", "88", "78", "44", "46"]


def test_gates_that_change_the_list():
    js = names(jobs(readiness=2, pai=4, stats=False))
    assert "r0c" not in js and "44" not in js and "pai" not in js and "13" in js
    assert not any(n.startswith("r") or n == "pai" for n in names(jobs(endpoints=set())))
    assert not set(names(jobs())) & {"2", "43", "86", "7"}
    assert names(jobs(stats=False))[-1] == "46"
    assert "46" not in names(jobs(advertised=ADVERTISED - {46}))
    assert "66" not in names(jobs(sleep_plan=False))


def test_raw_jobs_cover_every_other_advertised_type():
    js = jobs()
    raw = sync.raw_jobs(ADVERTISED, js)
    assert [j.code for j in raw] == [2, 7, 8, 11, 17, 30, 48, 63, 64, 85]
    covered = {j.code for j in js if j.kind in ("gdsp", "workouts")} | {6, 13} | {j.code for j in raw}
    assert covered == ADVERTISED
    # with PAI on GDSP 13 there is no raw 13
    assert 13 not in [j.code for j in sync.raw_jobs(ADVERTISED, jobs(pai=4))]
    assert sync.raw_jobs(None, js) == []


def test_select():
    js = jobs()
    assert names(sync.select(js, {"72", "pai"})) == ["72", "pai"]
    assert names(sync.select(js, {"readiness"})) == ["r08", "r0c", "r06"]
    assert names(sync.select(js, {"6"})) == ["5"]
    assert names(sync.select(js, {"13"})) == ["pai"]
    assert sync.select(js, None) == js
