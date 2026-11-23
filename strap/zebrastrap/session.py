"""Async channel session over a BLE link (ChannelSession.kt).

Writes go to HUAMI(0016). The strap notifies its ACKs and the version reply on 0016, and data chunks on 0017.
The phone writes its own ACKs to 0017. GDSP bulk history data arrives on HUAMI(0005).
"""

from __future__ import annotations

import asyncio
import contextlib
from typing import Callable, Optional

from . import transport
from .auth import ChannelVersion, ConnectCodec, Endpoints, ServiceList
from .crypto import ChannelCipher


def huami(short: int) -> str:
    return f"0000{short:04x}-0000-3512-2118-0009af100700"


def sig(short: int) -> str:
    return f"0000{short:04x}-0000-1000-8000-00805f9b34fb"


MAIN_SERVICE = sig(0xFEE0)
CHANNEL_CONTROL = huami(0x0016)
CHANNEL_DATA = huami(0x0017)
HISTORY_DATA = huami(0x0005)
LEGACY_BATTERY = huami(0x0006)
HR_MEASUREMENT = sig(0x2A37)
DEVICE_INFO = {
    "manufacturer": sig(0x2A29),
    "model": sig(0x2A24),
    "serial": sig(0x2A25),
    "hardware": sig(0x2A27),
    "firmware": sig(0x2A26),
    "software": sig(0x2A28),
}
DFU_SERVICE = huami(0x1530)  # firmware service: zebrastrap never touches it

ENCRYPTED_ACK_TIMEOUT = 2.0


class ProtocolError(Exception):
    pass


class ProtocolTimeout(Exception):
    """The strap did not reply in time."""


class _Fanout:
    """Delivers each item to every current subscriber queue."""

    def __init__(self) -> None:
        self._subs: list[tuple] = []

    def push(self, item) -> None:
        for q, keep, conv in list(self._subs):
            if keep(item):
                q.put_nowait(conv(item))

    @contextlib.contextmanager
    def subscribe(self, keep=lambda item: True, conv=lambda item: item):
        q: asyncio.Queue = asyncio.Queue()
        entry = (q, keep, conv)
        self._subs.append(entry)
        try:
            yield q
        finally:
            self._subs.remove(entry)


async def _first(q: asyncio.Queue, matches: Callable, timeout: float):
    async def wait():
        while True:
            item = await q.get()
            if matches(item):
                return item

    return await asyncio.wait_for(wait(), timeout)


class StrapLink:
    """The radio side. Subclasses write to 0016/0017 and call session.feed_control/feed_data on notifications."""

    payload_mtu: int = transport.DEFAULT_PAYLOAD_MTU

    async def write_control(self, data: bytes) -> None:  # 0016
        raise NotImplementedError

    async def write_data(self, data: bytes) -> None:  # 0017
        raise NotImplementedError


class ChannelSession:
    """One channel connection. Call start() once notifications are on, then auth.authenticate()."""

    def __init__(self, link: StrapLink, log: Callable[[str], None] = lambda s: None) -> None:
        self.link = link
        self.log = log
        self.version: Optional[ChannelVersion] = None
        self.services = ServiceList()
        self.cipher: Optional[ChannelCipher] = None
        self.pings = 0
        self._next_session_id = 0
        self._send_lock = asyncio.Lock()
        self._await_encrypted_acks = True
        self._control = _Fanout()
        self._messages = _Fanout()
        self._data_q: asyncio.Queue = asyncio.Queue()
        self._tasks: list[asyncio.Task] = []
        self.reassembler = transport.Reassembler(
            # Before the services list arrives, replies on the services and auth endpoints must be accepted.
            is_known_endpoint=lambda ep: ep in (Endpoints.SERVICES, Endpoints.AUTH) or self.services.has(ep),
            decrypt=self._decrypt,
            on_message=self._on_message,
        )
        self.reassembler.on_decrypt_failure = lambda e: self.log(f"dropped a message that did not decrypt: {e}")

    @property
    def is_authenticated(self) -> bool:
        return self.cipher is not None

    def _decrypt(self, sid: int, data: bytes, length: int) -> bytes:
        if self.cipher is None:
            raise ProtocolError("Encrypted message before auth")
        return self.cipher.decrypt(sid, data, length)

    # ---- notifications in -----------------------------------------------------------------------

    def feed_control(self, data: bytes) -> None:
        """A notification from 0016 (version reply, device ACKs)."""
        self._control.push(bytes(data))

    def feed_data(self, data: bytes) -> None:
        """A notification from 0017 (device->phone chunks). One consumer task keeps the ACK order."""
        self._data_q.put_nowait(bytes(data))

    def start(self) -> None:
        self._tasks.append(asyncio.ensure_future(self._reader()))

    async def stop(self) -> None:
        for t in self._tasks:
            t.cancel()
        for t in self._tasks:
            with contextlib.suppress(asyncio.CancelledError, Exception):
                await t
        self._tasks.clear()
        self.cipher = None

    async def _reader(self) -> None:
        while True:
            packet = await self._data_q.get()
            ack = self.reassembler.accept(packet)
            if ack is not None:
                try:
                    await self.link.write_data(ack)
                except Exception as e:
                    self.log(f"ack write failed: {e}")

    def _on_message(self, msg: transport.ChannelMessage) -> None:
        self._messages.push(msg)
        p = msg.payload
        if msg.endpoint == Endpoints.CONNECT and p and p[0] == ConnectCodec.PING[0]:
            if self.pings == 0:
                self.log("strap ping, answering pong")
            self.pings += 1
            self._tasks.append(asyncio.ensure_future(self._pong()))
        elif msg.endpoint == Endpoints.SERVICES:
            changed = self.services.apply_change(p)
            if changed is not None:
                self.log(f"services changed: {p.hex()}")
                self.services = changed

    async def _pong(self) -> None:
        try:
            await self.send(Endpoints.CONNECT, ConnectCodec.PONG, want_reply=False)
        except Exception as e:
            self.log(f"pong failed: {e}")

    @contextlib.contextmanager
    def subscribe(self, endpoint: int):
        """A queue of payloads that arrive on endpoint while the context is open."""
        with self._messages.subscribe(lambda m: m.endpoint == endpoint, lambda m: m.payload) as q:
            yield q

    # ---- sending ------------------------------------------------------------------------------

    async def control_request(self, data: bytes, matches: Callable[[bytes], bool], timeout: float) -> bytes:
        """Write raw bytes to 0016 and wait for a matching 0016 notification."""
        with self._control.subscribe() as q:
            await self.link.write_control(data)
            try:
                return await _first(q, matches, timeout)
            except asyncio.TimeoutError:
                raise ProtocolTimeout("No reply on 0016") from None

    async def send(self, endpoint: int, payload: bytes, want_reply: bool = True) -> None:
        """Send payload to endpoint, encrypted when the strap requires it for that endpoint."""
        async with self._send_lock:
            if await self._send_once(endpoint, payload, want_reply) == transport.ACK_PACKET_LOST:
                self.log(f"strap lost a message on {endpoint:04x}, sending it again")
                await self._send_once(endpoint, payload, want_reply)

    async def _send_once(self, endpoint: int, payload: bytes, want_reply: bool) -> Optional[int]:
        sid = self._next_session_id
        self._next_session_id = (self._next_session_id + 1) & 0xFF
        c = self.cipher
        encrypt = (c is not None and self.version is not None and self.version.encrypt_enable is True
                   and self.services.requires_encryption(endpoint))
        body = c.encrypt(sid, payload) if encrypt else payload
        chunks = transport.build_chunks(endpoint, sid, body, len(payload), encrypt, self.link.payload_mtu,
                                        want_reply or encrypt)
        if not encrypt or not self._await_encrypted_acks:
            for ch in chunks:
                await self.link.write_control(ch)
            return None
        # Zepp matches the ACK on the session id only. Listen before writing.
        with self._control.subscribe() as q:
            for ch in chunks:
                await self.link.write_control(ch)

            def is_ours(b: bytes) -> bool:
                a = transport.parse_device_ack(b)
                return a is not None and a.session_id == sid

            try:
                ack = transport.parse_device_ack(await _first(q, is_ours, ENCRYPTED_ACK_TIMEOUT))
            except asyncio.TimeoutError:
                ack = None
        if ack is None:
            self._await_encrypted_acks = False
            self.log(f"no ACK for an encrypted message on {endpoint:04x}; not waiting for them any more")
            return None
        if ack.status != transport.ACK_SUCCESS:
            self.log(f"strap ACKed {endpoint:04x} with status {ack.status}")
        return ack.status

    async def request(self, endpoint: int, payload: bytes, timeout: float = 10.0, want_reply: bool = True,
                      matches: Callable[[bytes], bool] = lambda b: True, log_payload: bool = True) -> bytes:
        """Send and wait for the first reply on the same endpoint that satisfies matches."""
        with self._messages.subscribe() as q:
            if log_payload:
                self.log(f"> {endpoint:04x} {payload.hex()}")
            await self.send(endpoint, payload, want_reply)
            try:
                msg = await _first(q, lambda m: m.endpoint == endpoint and matches(m.payload), timeout)
            except asyncio.TimeoutError:
                raise ProtocolTimeout(f"No reply on {endpoint:04x} in {timeout:g} s") from None
        if log_payload:
            self.log(f"< {endpoint:04x} {msg.payload.hex()}")
        return msg.payload


# ---------------------------------------------------------------------------------------------- bleak

STRAP_NAMES = ("Amazfit Helio Strap", "Stuttgart", "Amazfit Helio")


async def find_strap(address: Optional[str] = None, timeout: float = 15.0):
    from bleak import BleakScanner

    if address:
        dev = await BleakScanner.find_device_by_address(address, timeout=timeout)
    else:
        dev = await BleakScanner.find_device_by_filter(
            lambda d, a: (d.name or a.local_name or "").startswith(STRAP_NAMES), timeout=timeout)
    if dev is None:
        raise ProtocolError("strap not found (is it awake and not connected to another phone?)")
    return dev


class BleakStrapLink(StrapLink):
    """StrapLink over a connected bleak.BleakClient."""

    def __init__(self, client) -> None:
        self.client = client
        self.payload_mtu = transport.DEFAULT_PAYLOAD_MTU
        self._with_response: dict[str, bool] = {}

    def _response_for(self, uuid: str) -> bool:
        if uuid not in self._with_response:
            ch = self.client.services.get_characteristic(uuid)
            props = ch.properties if ch is not None else []
            # Android's default write type: with response when the characteristic offers it.
            self._with_response[uuid] = "write" in props
        return self._with_response[uuid]

    async def write_control(self, data: bytes) -> None:
        await self.client.write_gatt_char(CHANNEL_CONTROL, data, response=self._response_for(CHANNEL_CONTROL))

    async def write_data(self, data: bytes) -> None:
        await self.client.write_gatt_char(CHANNEL_DATA, data, response=self._response_for(CHANNEL_DATA))

    async def attach(self, session: ChannelSession,
                     on_history_data: Optional[Callable[[bytes], None]] = None) -> None:
        """Enable notifications on 0016, 0017 and (when on_history_data is given) 0005."""
        await self.client.start_notify(CHANNEL_CONTROL, lambda _, d: session.feed_control(bytes(d)))
        await self.client.start_notify(CHANNEL_DATA, lambda _, d: session.feed_data(bytes(d)))
        if on_history_data is not None:
            await self.client.start_notify(HISTORY_DATA, lambda _, d: on_history_data(bytes(d)))
