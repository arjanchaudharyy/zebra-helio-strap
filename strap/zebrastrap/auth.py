"""Version query, services list and the ECDH auth handshake (Auth.kt, Services.kt, ChannelSession.handshake).

Never log or print the auth key, the session key or the step-2 payload.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from typing import Optional

from . import crypto
from ._bytes import ByteReader


class Endpoints:
    """Channel endpoint ids (docs/protocol/01 section 4)."""

    SERVICES = 0x0000
    SETTINGS = 0x000A
    FIRMWARE = 0x000C  # never used by zebrastrap
    CONNECT = 0x0015
    REALTIME_ACTIVITY = 0x0016
    USER_INFO = 0x0017
    MOTOR_VIBRATE = 0x0018
    FIND_DEVICE = 0x001A
    OPEN_APP = 0x001D
    BATTERY = 0x0029
    DEVICE_INFO = 0x0043
    TIME = 0x0047
    HEART_RATE = 0x0048
    HISTORY = 0x004B
    PAI = 0x0022
    SLEEP = 0x0032
    READINESS = 0x0049
    AUTH = 0x0082


class ConnectCodec:
    """ConnectModule 0x0015: the strap's `03` ping is answered with a `04` pong."""

    MTU_QUERY = b"\x01"
    PING = b"\x03"
    PONG = b"\x04"


@dataclass(frozen=True)
class ChannelVersion:
    """Channel version reply on 0016: `02 version [encryptEnable] [supportBT] [featureFlag u16]`."""

    version: int
    encrypt_enable: Optional[bool]

    REQUEST = b"\x01"

    @staticmethod
    def parse(reply: bytes) -> Optional["ChannelVersion"]:
        if len(reply) < 2 or reply[0] != 0x02:
            return None
        return ChannelVersion(reply[1], (reply[2] != 0) if len(reply) >= 3 else None)


@dataclass
class ServiceList:
    """Supported endpoints and whether each requires encryption."""

    endpoints: dict[int, bool] = field(default_factory=dict)

    REQUEST = b"\x03"
    CHANGED = 0x05

    def has(self, endpoint: int) -> bool:
        return endpoint in self.endpoints

    def requires_encryption(self, endpoint: int) -> bool:
        return self.endpoints.get(endpoint) is True

    def apply_change(self, push: bytes) -> Optional["ServiceList"]:
        """Apply the strap's `05 op endpoint(u16) requireEncryption` push: op 1 adds, op 2 removes."""
        if len(push) < 4 or push[0] != self.CHANGED:
            return None
        endpoint = push[2] | (push[3] << 8)
        if push[1] == 1:
            d = dict(self.endpoints)
            d[endpoint] = len(push) > 4 and push[4] != 0
            return ServiceList(d)
        if push[1] == 2:
            d = dict(self.endpoints)
            d.pop(endpoint, None)
            return ServiceList(d)
        return None

    @staticmethod
    def parse(reply: bytes) -> "ServiceList":
        """Parse the v2 reply `04 count(u16) {endpoint u16, requireEncryption bool}*`."""
        if not reply or reply[0] != 0x04:
            raise ValueError("Not a v2 services reply")
        r = ByteReader(reply, 1)
        count = r.u16()
        d: dict[int, bool] = {}
        for _ in range(count):
            ep = r.u16()
            d[ep] = r.bool()
        if r.remaining:
            raise ValueError("Trailing bytes in services list")
        return ServiceList(d)

    def describe(self) -> str:
        return ", ".join(f"{ep:04x}{'*' if enc else ''}" for ep, enc in self.endpoints.items())


# ---------------------------------------------------------------------------------------------- auth

AUTH_ENDPOINT = Endpoints.AUTH
CMD_INFO = 0x00
CMD_KEY_EXCHANGE = 0x04
CMD_PROVE = 0x05
STATUS_SUCCESS = 1

_STATUS_NAMES = {
    0: "other error", 1: "success", 2: "invalid state", 3: "unknown command",
    4: "operation failed", 5: "a workout is in progress", 6: "invalid parameter",
    7: "no key on the strap (it was unbound)", 8: "signature verification failed",
    9: "API level insufficient", 0x20: "cannot save key", 0x21: "rejected on the strap",
    0x22: "public key hash failed", 0x23: "AES failed", 0x24: "out of memory",
    0x25: "binding ciphertext mismatch", 0x26: "wrong auth key",
    0x27: "ECDH private key failed", 0x28: "ECDH shared secret failed",
    0x41: "binding protection is on", 0x81: "local binding forbidden (cloud bind required)",
}


def describe(status: int) -> str:
    return _STATUS_NAMES.get(status, f"status 0x{status:02x}")


def strip_reply(command: int, reply: bytes) -> Optional[bytes]:
    """Strip the auth reply prefix `10 <cmd>`. Returns `[status][data...]`, or None."""
    if len(reply) >= 3 and reply[0] == 0x10 and reply[1] == command:
        return bytes(reply[2:])
    return None


class AuthError(Exception):
    def __init__(self, status: int, message: str) -> None:
        super().__init__(message)
        self.status = status


def parse_auth_key(text: Optional[str]) -> bytes:
    """A 16-byte key from 32 hex characters (optional 0x prefix). The error text never echoes the input."""
    if not text:
        raise ValueError("no auth key given (use --key or ZEBRA_STRAP_KEY)")
    clean = text.strip()
    if clean[:2].lower() == "0x":
        clean = clean[2:]
    if len(clean) != 32:
        raise ValueError("auth key must be 32 hex characters")
    try:
        return bytes.fromhex(clean)
    except ValueError:
        raise ValueError("auth key must be 32 hex characters") from None


# Zebra saves the strap's key here (0600) after "Get key from my Zepp account".
ZEBRA_KEY_FILE = os.path.expanduser("~/Library/Application Support/com.zeppbridge.ZeppBridge/data/strap.key")


def _key_from_zebra_file() -> Optional[str]:
    try:
        with open(ZEBRA_KEY_FILE) as f:
            for line in f:
                parts = line.rstrip("\n").split("\t")
                if len(parts) >= 3 and len(parts[2].strip()) >= 32:
                    return parts[2].strip()
    except OSError:
        return None
    return None


def key_from_env_or_arg(arg: Optional[str]) -> bytes:
    return parse_auth_key(arg or os.environ.get("ZEBRA_STRAP_KEY") or _key_from_zebra_file())


class AuthHandshake:
    """The already-bound handshake as a pure state machine (docs/protocol/01 section 3.4).

    Send step1_request, feed the stripped reply to handle_step1_reply (it returns the step-2 request),
    then check the step-2 status with handle_step2_reply, which yields the session ChannelCipher.
    """

    def __init__(self, auth_key: bytes, key_pair: Optional[crypto.KeyPair] = None) -> None:
        if len(auth_key) != 16:
            raise ValueError("Auth key must be 16 bytes")
        self._auth_key = bytes(auth_key)
        self._keys = key_pair or crypto.generate_key_pair()
        self._cipher: Optional[crypto.ChannelCipher] = None
        #: `04 02 00 02` + phone public key (52 bytes).
        self.step1_request = bytes([CMD_KEY_EXCHANGE, 0x02, 0x00, 0x02]) + self._keys.public_key

    def __repr__(self) -> str:
        return "AuthHandshake(<secret>)"

    def handle_step1_reply(self, reply: bytes) -> bytes:
        """reply is `status, random[16], devicePub[48]`. Returns the step-2 payload."""
        status = reply[0] if reply else 0
        if status != STATUS_SUCCESS:
            raise AuthError(status, f"Key exchange failed: {describe(status)}")
        if len(reply) < 1 + 16 + crypto.PUBLIC_KEY_BYTES:
            raise AuthError(0, f"Key exchange reply too short ({len(reply)})")
        challenge = reply[1:17]
        device_public = reply[17:17 + crypto.PUBLIC_KEY_BYTES]
        shared = crypto.shared_secret(self._keys.private_key, device_public)
        c = crypto.ChannelCipher(shared, self._auth_key)
        self._cipher = c
        return (bytes([CMD_PROVE]) + crypto.aes_encrypt(self._auth_key, challenge)
                + crypto.aes_encrypt(c.session_key, challenge))

    def handle_step2_reply(self, reply: bytes) -> crypto.ChannelCipher:
        status = reply[0] if reply else 0
        if status != STATUS_SUCCESS:
            raise AuthError(status, f"Authentication failed: {describe(status)}")
        if self._cipher is None:
            raise AuthError(0, "Step 2 before step 1")
        return self._cipher


# ---------------------------------------------------------------------------------------------- session steps
# These take a session.ChannelSession (duck-typed to avoid an import cycle).

VERSION_TIMEOUT = 5.0
SERVICES_TIMEOUT = 5.0
AUTH_TIMEOUT = 15.0


async def query_version(session) -> ChannelVersion:
    """Write `01` to 0016 and wait for the `02 ...` reply on 0016."""
    from .session import ProtocolError

    reply = await session.control_request(ChannelVersion.REQUEST, lambda b: len(b) > 0 and b[0] == 0x02,
                                          VERSION_TIMEOUT)
    v = ChannelVersion.parse(reply)
    if v is None:
        raise ProtocolError(f"Unexpected version reply {reply.hex()}")
    session.version = v
    return v


async def query_services(session) -> ServiceList:
    """Services request on endpoint 0x0000 (sent without ACK_REQ, as Zepp does)."""
    reply = await session.request(Endpoints.SERVICES, ServiceList.REQUEST, timeout=SERVICES_TIMEOUT,
                                  want_reply=False, matches=lambda b: len(b) > 0 and b[0] == 0x04)
    session.services = ServiceList.parse(reply)
    return session.services


async def _auth_request(session, command: int, payload: bytes) -> bytes:
    reply = await session.request(AUTH_ENDPOINT, payload, timeout=AUTH_TIMEOUT,
                                  matches=lambda b: strip_reply(command, b) is not None, log_payload=False)
    return strip_reply(command, reply)  # type: ignore[return-value]


async def authenticate(session, auth_key: bytes, key_pair: Optional[crypto.KeyPair] = None,
                       ask_info: bool = True) -> crypto.ChannelCipher:
    """Version query, services list, optional auth info, then the two-step ECDH auth.

    On success the session cipher is installed on the session. Raises AuthError on a rejected key.
    """
    from .session import ProtocolError, ProtocolTimeout

    v = await query_version(session)
    session.log(f"channel v{v.version}, encryptEnable={v.encrypt_enable}")
    if v.version != 2 or v.encrypt_enable is None:
        raise ProtocolError(f"Unsupported channel (v{v.version}, encryption {v.encrypt_enable})")
    services = await query_services(session)
    session.log(f"services: {services.describe()}")
    if not services.has(AUTH_ENDPOINT):
        raise ProtocolError("Strap does not offer the v2 auth endpoint")
    if ask_info:
        # Zepp asks for the authorization info first on every connect, and goes on without it.
        try:
            info = await _auth_request(session, CMD_INFO, bytes([CMD_INFO]))
            session.log(f"auth info {info.hex()}")
        except ProtocolTimeout as e:
            session.log(f"auth info: {e}")
    hs = AuthHandshake(auth_key, key_pair)
    step1 = await _auth_request(session, CMD_KEY_EXCHANGE, hs.step1_request)
    step2 = await _auth_request(session, CMD_PROVE, hs.handle_step1_reply(step1))
    cipher = hs.handle_step2_reply(step2)
    session.cipher = cipher
    session.log("authenticated")
    return cipher
