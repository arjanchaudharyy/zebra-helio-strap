"""v2 channel framing, reassembly and ACKs (Chunks.kt, docs/protocol/01 sections 2.2-2.3)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Callable, Optional

from ._bytes import ByteWriter, u16, u32

# chunk flags
FIRST = 0x01
LAST = 0x02
NEEDS_ACK = 0x04
ENCRYPTED = 0x08

# packet types
DATA = 0x03
ACK = 0x04

# ACK status codes
ACK_SUCCESS = 1
ACK_SESSION_MISMATCH = 2
ACK_PACKET_LOST = 3
ACK_BAD_FORMAT = 4
ACK_UNKNOWN_ENDPOINT = 5
ACK_CANNOT_DECRYPT = 12

FIRST_HEADER = 11
NEXT_HEADER = 5
REPLY_FREQ = 16
DEFAULT_PAYLOAD_MTU = 20


def build_chunks(
    endpoint: int,
    session_id: int,
    data: bytes,
    plain_length: int,
    encrypted: bool,
    payload_mtu: int = DEFAULT_PAYLOAD_MTU,
    want_reply: bool = True,
) -> list[bytes]:
    """Split one logical message into chunks for the phone->device characteristic (0016).

    data is the already-encrypted body when encrypted is set. plain_length is always the plaintext size.
    """
    if payload_mtu <= FIRST_HEADER:
        raise ValueError(f"Payload MTU too small: {payload_mtu}")
    chunks: list[bytes] = []
    offset = 0
    index = 0
    while True:
        first = index == 0  # as Kotlin: index wraps at 256
        room = payload_mtu - (FIRST_HEADER if first else NEXT_HEADER)
        n = min(room, len(data) - offset)
        last = offset + n >= len(data)
        flags = 0
        if first:
            flags |= FIRST
        if last:
            flags |= LAST
        if encrypted:
            flags |= ENCRYPTED
        if (last and want_reply) or (index + 1) % REPLY_FREQ == 0:
            flags |= NEEDS_ACK
        w = ByteWriter().u8(DATA).u8(flags).u8(0).u8(session_id).u8(index)
        if first:
            w.u32(plain_length).u16(endpoint)
        w.bytes(data[offset:offset + n])
        chunks.append(w.to_bytes())
        offset += n
        index = (index + 1) & 0xFF
        if offset >= len(data):
            return chunks


def build_ack(session_id: int, status: int, chunk_index: int) -> bytes:
    return bytes([ACK, 0, session_id & 0xFF, status & 0xFF, chunk_index & 0xFF])


@dataclass(frozen=True)
class DeviceAck:
    session_id: int
    status: int
    chunk_index: int


def parse_device_ack(packet: bytes) -> Optional[DeviceAck]:
    """The strap's ACK on 0016: `04 00 sess status idx`, or the 4-byte v1 form `04 sess status idx`."""
    if len(packet) < 4 or packet[0] != ACK:
        return None
    if len(packet) == 4:
        return DeviceAck(packet[1], packet[2], packet[3])
    if packet[1] != 0:
        return None
    return DeviceAck(packet[2], packet[3], packet[4])


@dataclass(frozen=True)
class ChannelMessage:
    endpoint: int
    session_id: int
    payload: bytes
    encrypted: bool


class Reassembler:
    """Reassemble device->phone chunks from 0017.

    Feed each notification to accept(). It returns the ACK bytes to write back to 0017 (or None),
    and hands completed messages to on_message.
    """

    def __init__(
        self,
        is_known_endpoint: Callable[[int], bool],
        decrypt: Callable[[int, bytes, int], bytes],
        on_message: Callable[[ChannelMessage], None],
    ) -> None:
        self.is_known_endpoint = is_known_endpoint
        self.decrypt = decrypt
        self.on_message = on_message
        self.on_decrypt_failure: Callable[[Exception], None] = lambda e: None
        self._session = -1
        self._endpoint = 0
        self._total = 0
        self._encrypted = False
        self._last_index = 0
        self._buf = bytearray()

    def _reset(self) -> None:
        self._session = -1
        self._buf = bytearray()

    def _payload_of(self, raw: bytes) -> Optional[bytes]:
        if not self._encrypted:
            return raw
        try:
            return self.decrypt(self._session, raw, self._total)
        except Exception as e:  # any decrypt failure drops the message
            self.on_decrypt_failure(e)
            return None

    def accept(self, packet: bytes) -> Optional[bytes]:
        packet = bytes(packet)
        if len(packet) < NEXT_HEADER or packet[0] != DATA or packet[2] != 0:
            return None
        flags = packet[1]
        session = packet[3]
        index = packet[4]
        needs_ack = bool(flags & NEEDS_ACK)

        def ack(status: int) -> Optional[bytes]:
            return build_ack(session, status, index) if (needs_ack or status != ACK_SUCCESS) else None

        if flags & FIRST:
            if len(packet) < FIRST_HEADER:
                return ack(ACK_BAD_FORMAT)
            self._session = session
            self._total = u32(packet, 5)
            self._endpoint = u16(packet, 9)
            self._encrypted = bool(flags & ENCRYPTED)
            self._last_index = index
            self._buf = bytearray()
            if not self.is_known_endpoint(self._endpoint):
                self._reset()
                return build_ack(session, ACK_UNKNOWN_ENDPOINT, index)
            body = packet[FIRST_HEADER:]
        else:
            # A continuation with no message in progress: nothing to ACK.
            if self._session == -1:
                return None
            if session != self._session:
                self._reset()
                return ack(ACK_SESSION_MISMATCH)
            if index != ((self._last_index + 1) & 0xFF):
                self._reset()
                return ack(ACK_PACKET_LOST)
            self._last_index = index
            if flags & ENCRYPTED:  # Zepp ORs the flag over every chunk
                self._encrypted = True
            body = packet[NEXT_HEADER:]
        self._buf += body

        expected = (self._total + 8 + 15) // 16 * 16 if self._encrypted else self._total
        if len(self._buf) > expected:
            self._reset()
            return ack(ACK_BAD_FORMAT)
        if flags & LAST:
            if len(self._buf) != expected:
                self._reset()
                return ack(ACK_BAD_FORMAT)
            raw = bytes(self._buf)
            payload = self._payload_of(raw)
            if payload is None:
                # Zepp ACKs these as received and drops them silently (02 section 4.3).
                self._reset()
                return ack(ACK_SUCCESS)
            message = ChannelMessage(self._endpoint, self._session, payload, self._encrypted)
            self._reset()
            reply = ack(ACK_SUCCESS)
            self.on_message(message)
            return reply
        return ack(ACK_SUCCESS)
