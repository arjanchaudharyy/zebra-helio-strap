"""Little-endian helpers. The strap's wire format is little-endian throughout (Bytes.kt)."""

from __future__ import annotations


class ShortRead(IndexError):
    """A read went past the end of the buffer (a truncated record)."""


def hex_to_bytes(s: str) -> bytes:
    clean = "".join(c for c in s if not c.isspace() and c != ":")
    if clean.startswith("0x") or clean.startswith("0X"):
        clean = clean[2:]
    if len(clean) % 2:
        raise ValueError("Odd hex length")
    return bytes.fromhex(clean)


def u16(b: bytes, at: int) -> int:
    return b[at] | (b[at + 1] << 8)


def u32(b: bytes, at: int) -> int:
    if len(b) < at + 4:
        _short(at, 4, len(b))
    return int.from_bytes(b[at:at + 4], "little")


def _short(at: int, n: int, size: int):
    raise ShortRead(f"read of {n} at {at} past end {size}")


def signed(v: int, bits: int) -> int:
    v &= (1 << bits) - 1
    return v - (1 << bits) if v >> (bits - 1) else v


class ByteWriter:
    """Growable buffer for little-endian payloads. Values are masked, so negative numbers work."""

    def __init__(self) -> None:
        self._out = bytearray()

    def u8(self, v: int) -> "ByteWriter":
        self._out.append(v & 0xFF)
        return self

    def u16(self, v: int) -> "ByteWriter":
        self._out += (v & 0xFFFF).to_bytes(2, "little")
        return self

    def u32(self, v: int) -> "ByteWriter":
        self._out += (v & 0xFFFFFFFF).to_bytes(4, "little")
        return self

    def u64(self, v: int) -> "ByteWriter":
        self._out += (v & 0xFFFFFFFFFFFFFFFF).to_bytes(8, "little")
        return self

    def bytes(self, b: bytes) -> "ByteWriter":
        self._out += b
        return self

    def to_bytes(self) -> bytes:
        return bytes(self._out)


class ByteReader:
    """Sequential little-endian reader. Raises ShortRead on a read past the end."""

    def __init__(self, data: bytes, pos: int = 0) -> None:
        self.data = bytes(data)
        self.pos = pos

    @property
    def remaining(self) -> int:
        return len(self.data) - self.pos

    def bytes(self, n: int) -> bytes:
        if n < 0 or self.pos + n > len(self.data):
            _short(self.pos, n, len(self.data))
        out = self.data[self.pos:self.pos + n]
        self.pos += n
        return out

    def u8(self) -> int:
        return self.bytes(1)[0]

    def s8(self) -> int:
        return signed(self.u8(), 8)

    def u16(self) -> int:
        return int.from_bytes(self.bytes(2), "little")

    def s16(self) -> int:
        return signed(self.u16(), 16)

    def u32(self) -> int:
        return int.from_bytes(self.bytes(4), "little")

    def s32(self) -> int:
        return signed(self.u32(), 32)

    def bool(self) -> bool:
        return self.u8() != 0

    def rest(self) -> bytes:
        return self.bytes(self.remaining)
