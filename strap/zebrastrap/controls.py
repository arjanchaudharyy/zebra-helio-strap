"""Vibration (0x0018), find device (0x001A) and settings (0x000A) codecs (FindDevice.kt, Settings.kt).

The async helpers need an authenticated session.ChannelSession.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from ._bytes import ByteReader, ByteWriter
from .auth import Endpoints

# ============================================================================================== motor / find


class MotorVibrate:
    """MotorVibrateModule 0x0018: `03 scene custom test nPairs {u16 LE}*`, answered `04 01`.

    The pattern alternates vibrate and pause milliseconds, zero-padded to an even count.
    """

    SET = 0x03
    SCENE_APP_NOTIFY = 0
    SCENE_INCOMING_CALL = 1

    @staticmethod
    def play(pattern: list[int], scene: int = 0) -> bytes:
        if not pattern or not all(0 <= p <= 0xFFFF for p in pattern):
            raise ValueError(f"bad pattern {pattern}")
        padded = list(pattern) + ([0] if len(pattern) % 2 else [])
        w = ByteWriter().u8(MotorVibrate.SET).u8(scene).u8(1).u8(1).u8(len(padded) // 2)
        for p in padded:
            w.u16(p)
        return w.to_bytes()

    @staticmethod
    def is_ok(reply: bytes) -> bool:
        return len(reply) >= 2 and reply[0] == MotorVibrate.SET + 1 and reply[1] == 1


class FindDevice:
    """FindDeviceModule 0x001A. BUZZ starts buzzing until STOP. START (`05 01`) is Zepp's continuous find."""

    BUZZ = b"\x03"
    START = b"\x05\x01"
    STOP = b"\x06"
    FOUND = 0x07
    SHORT_BUZZ_MS = 1_500
    FIND_TIMEOUT_MS = 60_000

    @staticmethod
    def is_buzz_ok(reply: bytes) -> bool:
        return len(reply) >= 2 and reply[0] == 0x04 and reply[1] == 1


# ============================================================================================== settings

TYPE_I16 = 0x01
TYPE_I16_LIST = 0x02
TYPE_I32 = 0x03
TYPE_I32_LIST = 0x04
TYPE_BOOL = 0x0B
TYPE_ENUM = 0x10
TYPE_MULTI = 0x11
TYPE_STRING = 0x20
TYPE_STRING_ENUM = 0x21
TYPE_TIME = 0x30
TYPE_I64 = 0x40
TYPE_I32_B = 0x50


@dataclass(frozen=True)
class Setting:
    key: int
    type: int
    raw: bytes
    number: Optional[int]
    choices: Optional[tuple] = None
    flags: tuple = ()

    @property
    def bool(self) -> Optional[bool]:
        return (self.number != 0) if self.type == TYPE_BOOL else None

    @property
    def text(self) -> Optional[str]:
        if self.type in (TYPE_STRING, TYPE_STRING_ENUM):
            return self.raw.decode("utf-8", "replace").split("\x00", 1)[0]
        return None


@dataclass(frozen=True)
class SettingsProtocol:
    version: int
    groups: tuple


@dataclass
class SettingsGroup:
    group: int
    version: int
    settings: dict = field(default_factory=dict)

    def __getitem__(self, key: int) -> Optional[Setting]:
        return self.settings.get(key)

    def updated_with(self, update: "SettingsGroup") -> Optional["SettingsGroup"]:
        if update.group != self.group:
            return None
        return SettingsGroup(self.group, update.version, {**self.settings, **update.settings})


class SettingsCodec:
    """Query `03 withOptions group n keys...`, reply `04 code group grpVer withOptions n {key type value [options]}`.
    Write `05 group grpVer 00 n {key type value}` with keys ascending, answered `06 01`."""

    VERSION = 0x01
    QUERY = 0x03
    REPLY = 0x04
    WRITE = 0x05

    GROUP_DEVICE_INFORMATION = 0x00
    GROUP_HEALTH = 0x08
    GROUP_GENERAL = 0x0A
    KEY_HR_INTERVAL = 0x01
    KEY_HIGH_HR_ALERT = 0x02
    KEY_LOW_HR_ALERT = 0x03
    KEY_ACTIVITY_HR = 0x04
    KEY_STRESS = 0x13
    KEY_LOW_SPO2_ALERT = 0x32
    KEY_CALORIE_TARGET = 0x53
    KEY_RESTART = 0x2D
    HR_CONTINUOUS = 0xFE

    version_request = b"\x01"

    @staticmethod
    def parse_version(reply: bytes) -> Optional[SettingsProtocol]:
        if len(reply) < 2 or reply[0] != SettingsCodec.VERSION + 1:
            return None
        version = reply[1]
        if len(reply) == 2:
            return SettingsProtocol(version, ())
        n = reply[2]
        if len(reply) < 3 + n:
            return None
        return SettingsProtocol(version, tuple(reply[3:3 + n]))

    @staticmethod
    def flag_bytes(protocol_version: Optional[int]) -> int:
        return min(max((protocol_version if protocol_version is not None else 1) - 3, 0), 3)

    @staticmethod
    def query(group: int, keys: list[int] = (), with_options: bool = True) -> bytes:  # type: ignore[assignment]
        keys = list(keys)
        return bytes([SettingsCodec.QUERY, 1 if with_options else 0, group, len(keys), *keys])

    @staticmethod
    def reply_code(reply: bytes) -> Optional[int]:
        return reply[1] if len(reply) >= 2 and reply[0] == SettingsCodec.REPLY else None

    @staticmethod
    def reply_group(reply: bytes) -> Optional[int]:
        return reply[2] if SettingsCodec.reply_code(reply) == 1 and len(reply) >= 3 else None

    @staticmethod
    def is_reply_to(group: int, reply: bytes) -> bool:
        code = SettingsCodec.reply_code(reply)
        if code is None:
            return False
        return code != 1 or SettingsCodec.reply_group(reply) == group

    @staticmethod
    def parse(reply: bytes, protocol_version: Optional[int] = None) -> Optional[SettingsGroup]:
        trailer = SettingsCodec.flag_bytes(protocol_version)
        if SettingsCodec.reply_code(reply) != 1 or len(reply) < 6:
            return None
        r = ByteReader(reply, 2)
        try:
            group, version, options, n = r.u8(), r.u8(), r.u8() != 0, r.u8()
            out: dict[int, Setting] = {}
            for _ in range(n):
                key, typ = r.u8(), r.u8()
                start = r.pos
                number: Optional[int] = None
                if typ == TYPE_I16:
                    number = r.s16()
                elif typ in (TYPE_I32, TYPE_I32_B):
                    number = r.s32()
                elif typ in (TYPE_BOOL, TYPE_ENUM):
                    number = r.u8()
                elif typ == TYPE_I64:
                    number = r.u32() | (r.u32() << 32)
                elif typ == TYPE_TIME:
                    h, m = r.u8(), r.u8()
                    number = h * 60 + m
                elif typ == TYPE_I16_LIST:
                    r.bytes(r.u8() * 2)
                elif typ == TYPE_I32_LIST:
                    r.bytes(r.u8() * 4)
                elif typ == TYPE_MULTI:
                    r.bytes(r.u8())
                elif typ in (TYPE_STRING, TYPE_STRING_ENUM):
                    _cstring(r)
                else:
                    return None  # unknown type: the rest cannot be framed
                raw = reply[start:r.pos]
                choices = None
                if options:
                    if typ == TYPE_I16:
                        r.bytes(4)
                    elif typ == TYPE_I32:
                        r.bytes(8)
                    elif typ == TYPE_ENUM:
                        choices = tuple(r.u8() for _ in range(r.u8()))
                    elif typ == TYPE_MULTI:
                        r.bytes(r.u8())
                    elif typ == TYPE_I16_LIST:
                        r.bytes(6)
                    elif typ == TYPE_I32_LIST:
                        r.bytes(10)
                    elif typ == TYPE_STRING:
                        r.bytes(1)
                    elif typ == TYPE_STRING_ENUM:
                        r.bytes(1)
                        for _ in range(r.u8()):
                            _cstring(r)
                flags = tuple(r.u8() for _ in range(trailer))
                out[key] = Setting(key, typ, bytes(raw), number, choices, flags)
        except IndexError:
            return None
        return SettingsGroup(group, version, out)

    @staticmethod
    def write(group: int, version: int, settings: list[Setting]) -> bytes:
        w = ByteWriter().u8(SettingsCodec.WRITE).u8(group).u8(version).u8(0).u8(len(settings))
        for s in sorted(settings, key=lambda s: s.key):
            w.u8(s.key).u8(s.type).bytes(s.raw)
        return w.to_bytes()

    @staticmethod
    def enum_setting(key: int, value: int) -> Setting:
        return Setting(key, TYPE_ENUM, bytes([value & 0xFF]), value)

    @staticmethod
    def bool_setting(key: int, on: bool) -> Setting:
        return Setting(key, TYPE_BOOL, bytes([1 if on else 0]), 1 if on else 0)

    @staticmethod
    def int_setting(key: int, typ: int, value: int) -> Optional[Setting]:
        if typ == TYPE_I16:
            return Setting(key, typ, (value & 0xFFFF).to_bytes(2, "little"), value)
        if typ == TYPE_I32:
            return Setting(key, typ, (value & 0xFFFFFFFF).to_bytes(4, "little"), value)
        return None

    @staticmethod
    def is_write_success(reply: bytes) -> bool:
        return len(reply) == 2 and reply[0] == SettingsCodec.WRITE + 1 and reply[1] == 1

    @staticmethod
    def hr_interval_write(health: SettingsGroup, value: int) -> bytes:
        """The write that sets HEALTH key 1 (HR interval: 0 off, 0xFE continuous, else minutes).

        The value must be one of the choices the strap reported, when it reported any.
        """
        current = health[SettingsCodec.KEY_HR_INTERVAL]
        if health.group != SettingsCodec.GROUP_HEALTH:
            raise ValueError("not the HEALTH group")
        if current is not None and current.choices and value not in current.choices:
            raise ValueError(f"HR interval {value} not in the strap's choices {list(current.choices)}")
        if current is not None and current.type != TYPE_ENUM:
            raise ValueError(f"HR interval has type 0x{current.type:02x}, expected an enum")
        return SettingsCodec.write(SettingsCodec.GROUP_HEALTH, health.version,
                                   [SettingsCodec.enum_setting(SettingsCodec.KEY_HR_INTERVAL, value)])


def enum_write(grp: SettingsGroup, key: int, value: int) -> bytes:
    """Write one choice-list (enum) setting. Refuses any value the strap did not offer as a choice."""
    current = grp[key]
    if current is None or current.type != TYPE_ENUM or not current.choices:
        raise ValueError(f"group 0x{grp.group:02x} key 0x{key:02x} is not a choice setting")
    if value not in current.choices:
        raise ValueError(f"{value} not in the strap's choices {list(current.choices)}")
    return SettingsCodec.write(grp.group, grp.version, [SettingsCodec.enum_setting(key, value)])


def _cstring(r: ByteReader) -> None:
    while r.u8() != 0:
        pass


# ============================================================================================== async helpers


async def vibrate(session, pattern: list[int], scene: int = MotorVibrate.SCENE_APP_NOTIFY) -> bool:
    reply = await session.request(Endpoints.MOTOR_VIBRATE, MotorVibrate.play(pattern, scene),
                                  matches=lambda b: len(b) > 0 and b[0] == MotorVibrate.SET + 1)
    return MotorVibrate.is_ok(reply)


async def find_device_start(session) -> bool:
    reply = await session.request(Endpoints.FIND_DEVICE, FindDevice.BUZZ, matches=lambda b: len(b) > 0 and b[0] == 4)
    return FindDevice.is_buzz_ok(reply)


async def find_device_stop(session) -> None:
    await session.send(Endpoints.FIND_DEVICE, FindDevice.STOP)


async def read_settings_group(session, group: int) -> Optional[SettingsGroup]:
    ver_reply = await session.request(Endpoints.SETTINGS, SettingsCodec.version_request,
                                      matches=lambda b: len(b) > 0 and b[0] == SettingsCodec.VERSION + 1)
    proto = SettingsCodec.parse_version(ver_reply)
    reply = await session.request(Endpoints.SETTINGS, SettingsCodec.query(group),
                                  matches=lambda b: SettingsCodec.is_reply_to(group, b))
    return SettingsCodec.parse(reply, proto.version if proto else None)


async def set_choice(session, group: int, key: int, value: int) -> bool:
    grp = await read_settings_group(session, group)
    if grp is None:
        raise RuntimeError(f"could not read settings group 0x{group:02x}")
    reply = await session.request(Endpoints.SETTINGS, enum_write(grp, key, value),
                                  matches=lambda b: len(b) > 0 and b[0] == SettingsCodec.WRITE + 1)
    return SettingsCodec.is_write_success(reply)


async def set_hr_interval(session, value: int) -> bool:
    health = await read_settings_group(session, SettingsCodec.GROUP_HEALTH)
    if health is None:
        raise RuntimeError("could not read the HEALTH settings group")
    reply = await session.request(Endpoints.SETTINGS, SettingsCodec.hr_interval_write(health, value),
                                  matches=lambda b: len(b) > 0 and b[0] == SettingsCodec.WRITE + 1)
    return SettingsCodec.is_write_success(reply)
