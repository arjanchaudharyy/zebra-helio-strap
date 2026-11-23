"""Command line: python -m zebrastrap.cli info|live|sync|vibrate [--key HEX].

info and live need no key. sync and vibrate read the key from --key or ZEBRA_STRAP_KEY. The key is never printed.
"""

from __future__ import annotations

import argparse
import asyncio
import sys
from datetime import datetime
from pathlib import Path
from typing import Optional

from . import auth, controls, history, sync
from .session import (DEVICE_INFO, HISTORY_DATA, HR_MEASUREMENT, LEGACY_BATTERY, BleakStrapLink, ChannelSession,
                      ProtocolError, find_strap)

# Zebra reads these files (Heart page, Coach). Same data dir as the app.
OUT_DIR = Path.home() / "Library/Application Support/com.zeppbridge.ZeppBridge/data/strap"


def _logger(verbose: bool):
    if not verbose:
        return lambda s: None
    return lambda s: print(f"  . {s}", file=sys.stderr, flush=True)


def _zone():
    return datetime.now().astimezone().tzinfo


async def _connect(address: Optional[str]):
    from bleak import BleakClient

    dev = await find_strap(address)
    client = BleakClient(dev, timeout=20)
    await client.connect()
    return dev, client


# ---------------------------------------------------------------------------------------------- info


async def cmd_info(args) -> int:
    log = _logger(args.verbose)
    dev, client = await _connect(args.address)
    try:
        print(f"device: {dev.name} ({dev.address})")
        print(f"att mtu: {getattr(client, 'mtu_size', '?')}")
        for label, uuid in DEVICE_INFO.items():
            ch = client.services.get_characteristic(uuid)
            if ch is None or "read" not in ch.properties:
                continue
            try:
                raw = bytes(await client.read_gatt_char(ch))
                text = raw.decode() if raw and all(32 <= b < 127 for b in raw) else raw.hex()
                print(f"{label}: {text}")
            except Exception as e:
                print(f"{label}: read failed ({type(e).__name__})")
        ch = client.services.get_characteristic(LEGACY_BATTERY)
        if ch is not None and "read" in ch.properties:
            try:
                raw = bytes(await client.read_gatt_char(ch))
                if len(raw) >= 2:
                    charging = (raw[2] in (1, 2)) if (raw[0] & 1 and len(raw) > 2) else None
                    print(f"battery: {raw[1]}% charging={charging} (raw {raw.hex()})")
            except Exception as e:
                print(f"battery: read failed ({type(e).__name__})")
        hd = client.services.get_characteristic(HISTORY_DATA)
        print(f"history data 0005: {sorted(hd.properties) if hd else 'missing'}")
        # Channel queries: version on 0016 and the services list on endpoint 0x0000. No auth traffic.
        link = BleakStrapLink(client)
        session = ChannelSession(link, log)
        await link.attach(session)
        session.start()
        try:
            v = await auth.query_version(session)
            print(f"channel: v{v.version}, encryptEnable={v.encrypt_enable}")
            services = await auth.query_services(session)
            print(f"services ({len(services.endpoints)}, * = encrypted): {services.describe()}")
            print(f"history control via channel 0x004B: {services.has(auth.Endpoints.HISTORY)}")
        except Exception as e:
            print(f"channel query failed: {type(e).__name__}: {e}")
        finally:
            await session.stop()
    finally:
        await client.disconnect()
    return 0


# ---------------------------------------------------------------------------------------------- live


def parse_hr_measurement(value: bytes):
    """Standard BLE 2A37: (bpm or None, off_body)."""
    if len(value) < 2:
        return None, False
    flags = value[0]
    bpm = value[1] | (value[2] << 8) if (flags & 1 and len(value) >= 3) else value[1]
    off_body = bool(flags & 0x04) and not (flags & 0x02)
    return (bpm if 1 <= bpm <= 250 else None), off_body


async def cmd_live(args) -> int:
    dev, client = await _connect(args.address)
    samples = []
    try:
        print(f"device: {dev.name}. Listening to 2A37 for {args.seconds} s")

        def on_hr(_, data: bytearray):
            bpm, off = parse_hr_measurement(bytes(data))
            samples.append(bpm)
            stamp = datetime.now().strftime("%H:%M:%S")
            print(f"{stamp} hr={bpm} bpm{' (off body)' if off else ''} raw={bytes(data).hex()}", flush=True)

        await client.start_notify(HR_MEASUREMENT, on_hr)
        await asyncio.sleep(args.seconds)
        try:
            await client.stop_notify(HR_MEASUREMENT)
        except Exception:
            pass
    finally:
        await client.disconnect()
    valid = [s for s in samples if s]
    print(f"samples: {len(samples)}" + (f", mean {sum(valid) / len(valid):.0f} bpm" if valid else ""))
    return 0


# ---------------------------------------------------------------------------------------------- authenticated


async def _authed(args, on_history_data=None):
    key = args.key_bytes
    log = _logger(args.verbose)
    dev, client = await _connect(args.address)
    link = BleakStrapLink(client)
    session = ChannelSession(link, log)
    await link.attach(session, on_history_data)
    session.start()
    try:
        await auth.authenticate(session, key)
    except BaseException:
        await session.stop()
        await client.disconnect()
        raise
    return client, session


async def cmd_vibrate(args) -> int:
    pattern = [int(x) for x in args.pattern.split(",")]
    client, session = await _authed(args)
    try:
        ok = await controls.vibrate(session, pattern)
        print("vibrate: ok" if ok else "vibrate: strap refused")
        return 0 if ok else 1
    finally:
        await session.stop()
        await client.disconnect()


# Named haptic patterns (on, off, on ... in ms). Short and distinct so they can be told apart by feel.
PATTERNS = {
    "one": [450],                             # Claude Code finished
    "two": [300, 250, 300],                   # Claude Code needs your input
    "done": [250, 200, 250],                  # two short: a task finished
    "attention": [120, 100, 120, 100, 120],   # three quick: something needs you
    "error": [900],                           # one long: something failed
    "bedtime": [400, 300, 400, 300, 400],     # three medium: time to wind down
    "breathe": [4000, 4000, 4000, 4000, 4000, 4000],  # slow: buzz = breathe in, pause = breathe out
    "find": [700, 300, 700, 300, 700, 300, 700],  # four long: find the strap
    "wake": [600, 400, 600, 400, 600, 1200, 600, 400, 600, 400, 600, 1200, 900],  # rising: wake-up alarm
    "hello": [80, 80, 80, 80, 80, 80, 300],   # a little drum roll
    "alert": [150, 100, 150, 100, 150, 600, 150, 100, 150, 100, 150],  # 3 quick, pause, 3 quick: a health alert
}


async def cmd_buzz(args) -> int:
    name = (args.name or "done").lower()
    if name not in PATTERNS:
        print(f"unknown pattern {name!r}; choose from: {', '.join(PATTERNS)}")
        return 2
    args.pattern = ",".join(str(x) for x in PATTERNS[name])
    return await cmd_vibrate(args)


async def cmd_watch(args) -> int:
    """Run the health rules over downloaded data; buzz once if there is any new alert."""
    from . import watch
    out = Path(args.out).expanduser()
    new = watch.check(out)
    for a in new:
        print(f"ALERT {a['kind']}: {a['text']}")
    if not new:
        print("watch: no new alerts")
        return 0
    if getattr(args, "key_bytes", None):
        args.pattern = ",".join(str(x) for x in PATTERNS["alert"])
        return await cmd_vibrate(args)
    return 0


async def cmd_sync(args) -> int:
    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    zone = _zone()
    log = _logger(args.verbose)
    gdsp: Optional[history.GdspClient] = None

    def on_data(p: bytes):
        if gdsp is not None:
            gdsp.on_data(p)

    client, session = await _authed(args, on_data)
    try:
        if not session.services.has(auth.Endpoints.HISTORY):
            print("strap does not list channel 0x004B; the HUAMI(0004) control path is not ported")
            return 1

        async def send(b: bytes):
            await session.send(auth.Endpoints.HISTORY, b)

        gdsp = history.GdspClient(send, log=log)
        with session.subscribe(auth.Endpoints.HISTORY) as q:
            async def pump():
                while True:
                    gdsp.on_control(await q.get())

            pump_task = asyncio.ensure_future(pump())
            try:
                runner = sync.SyncRunner(session, gdsp, out, zone, log, days=args.days)
                jobs, advertised = await runner.plan(zepp_only=getattr(args, "zepp_only", False))
                wanted = {t.strip().lower() for t in args.types.split(",")} if args.types else None
                jobs = sync.select(jobs, wanted)
                print(f"strap offers {sorted(advertised) if advertised else advertised}")
                print("jobs: " + " ".join(j.label for j in jobs))
                reports = await runner.run(jobs)
                for r in reports:
                    status = "ok" if r.ok else "FAILED"
                    print(f"{r.job.label:>7s} {r.fetched:6d} fetched {r.written:6d} new  {status}"
                          + (f" -> {out / r.file}" if r.file else ""))
                if runner.fast_recharge:
                    print("strap is in fast-recharge mode: sync stopped")
                return 0 if all(r.ok for r in reports) else 1
            finally:
                pump_task.cancel()
    finally:
        await session.stop()
        await client.disconnect()


# ---------------------------------------------------------------------------------------------- main


async def cmd_settings(args) -> int:
    """Print the health and workout settings groups as JSON (one object per key)."""
    import json
    from .controls import read_settings_group
    client, session = await _authed(args)
    try:
        out = {}
        for g in (0x08, 0x09):
            grp = await asyncio.wait_for(read_settings_group(session, g), 10)
            if grp is None:
                continue
            for k, v in sorted(grp.settings.items()):
                raw = v.raw
                nums = None
                if v.number is None and len(raw) >= 3 and (len(raw) - 1) == raw[0] * 2:
                    nums = [int.from_bytes(raw[1 + 2 * i:3 + 2 * i], "little") for i in range(raw[0])]
                out[f"{g:02x}:{k:02x}"] = {"value": v.number, "list": nums, "choices": list(v.choices) if v.choices else None}
        print(json.dumps(out))
        return 0
    finally:
        await session.stop()
        await client.disconnect()


async def cmd_set(args) -> int:
    """Set one choice setting: zebrastrap set 08:02 --value 140."""
    from .controls import set_choice
    g, k = (int(x, 16) for x in (args.name or "").split(":"))
    client, session = await _authed(args)
    try:
        ok = await set_choice(session, g, k, int(args.value))
        print("ok" if ok else "refused")
        return 0 if ok else 1
    finally:
        await session.stop()
        await client.disconnect()


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="zebrastrap", description="Amazfit Helio Strap BLE client")
    p.add_argument("command", choices=["info", "live", "sync", "vibrate", "buzz", "watch", "settings", "set"])
    p.add_argument("name", nargs="?", help="buzz: done | attention | error | bedtime | breathe | hello")
    p.add_argument("--key", help="16-byte auth key as 32 hex chars (or env ZEBRA_STRAP_KEY)")
    p.add_argument("--address", help="BLE address or macOS UUID (default: scan by name)")
    p.add_argument("--seconds", type=float, default=30, help="live: how long to listen")
    p.add_argument("--days", type=float, help="sync: ignore stored cursors and start this many days back")
    p.add_argument("--types", help="sync: comma-separated jobs: GDSP codes, pai, readiness, r08, r0c, r06")
    p.add_argument("--all", action="store_true", default=True,
                   help="sync (default): Zepp's job list, then every other advertised type as raw bytes")
    p.add_argument("--zepp-only", action="store_true", help="sync: Zepp's job list only, no raw types")
    p.add_argument("--out", default=str(OUT_DIR), help="sync: output directory")
    p.add_argument("--pattern", default="200,150,200", help="vibrate: on,off,on... milliseconds")
    p.add_argument("--value", help="set: the new value (must be one of the strap's choices)")
    p.add_argument("-v", "--verbose", action="store_true")
    args = p.parse_args(argv)

    if args.command in ("sync", "vibrate", "buzz", "watch", "settings", "set"):
        # Refuse before any radio traffic. The message never contains the key.
        try:
            args.key_bytes = auth.key_from_env_or_arg(args.key)
        except ValueError as e:
            print(f"error: {e}", file=sys.stderr)
            return 2
    args.key = None  # drop the text form

    handlers = {"info": cmd_info, "live": cmd_live, "sync": cmd_sync, "vibrate": cmd_vibrate, "buzz": cmd_buzz, "watch": cmd_watch, "settings": cmd_settings, "set": cmd_set}
    try:
        return asyncio.run(handlers[args.command](args))
    except auth.AuthError as e:
        print(f"auth failed: {e}", file=sys.stderr)
        return 3
    except ProtocolError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    except ValueError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
