"""AuthHandshakeTest.kt and ChannelSessionTest.kt, plus fixed handshake vectors.

The Kotlin source has no captures from a real strap. The fixed vectors below come from OpenSSL key pairs
(see test_crypto.py) and an independent AES/CRC build. They pin the exact bytes of steps 1 and 2.
"""

import asyncio

import pytest

from zebrastrap import auth, crypto, transport
from zebrastrap.auth import AuthError, AuthHandshake, Endpoints
from zebrastrap.crypto import ChannelCipher
from zebrastrap.session import ChannelSession, StrapLink
from zebrastrap.tests.test_crypto import A_PRIV, A_PUB, B_PRIV, B_PUB, sec1_to_wire

AUTH_KEY = bytes.fromhex("3c4a6f0e9b1d22e7a8c5f01b6d9e4a77")
CHALLENGE = bytes((i * 5 + 1) & 0xFF for i in range(16))

# ---- pure state machine ---------------------------------------------------------------------------


class FakeStrap:
    """Device side of doc 01 section 3.4."""

    def __init__(self, key: bytes, keys=None, challenge=bytes((i * 7) & 0xFF for i in range(16))):
        self.key = key
        self.keys = keys or crypto.generate_key_pair()
        self.challenge = challenge
        self.cipher = None

    def on_step1(self, req: bytes) -> bytes:
        assert len(req) == 52 and req[:4] == bytes([4, 2, 0, 2])
        self.cipher = ChannelCipher(crypto.shared_secret(self.keys.private_key, req[4:52]), self.key)
        return b"\x01" + self.challenge + self.keys.public_key

    def on_step2(self, req: bytes) -> bytes:
        ok = (req[0] == 5 and req[1:17] == crypto.aes_encrypt(self.key, self.challenge)
              and req[17:33] == crypto.aes_encrypt(self.cipher.session_key, self.challenge))
        return bytes([1 if ok else 0x26])


def test_handshake_agrees_on_session_key():
    strap = FakeStrap(bytes.fromhex("00112233445566778899aabbccddeeff"))
    hs = AuthHandshake(strap.key)
    step2 = hs.handle_step1_reply(strap.on_step1(hs.step1_request))
    assert len(step2) == 33
    assert hs.handle_step2_reply(strap.on_step2(step2)).session_key == strap.cipher.session_key


def test_wrong_key_is_reported():
    strap = FakeStrap(bytes.fromhex("00112233445566778899aabbccddeeff"))
    hs = AuthHandshake(bytes.fromhex("ffeeddccbbaa99887766554433221100"))
    step2 = hs.handle_step1_reply(strap.on_step1(hs.step1_request))
    with pytest.raises(AuthError) as e:
        hs.handle_step2_reply(strap.on_step2(step2))
    assert e.value.status == 0x26 and "wrong auth key" in str(e.value)


def test_step1_refusal_and_short_reply():
    hs = AuthHandshake(AUTH_KEY)
    with pytest.raises(AuthError) as e:
        hs.handle_step1_reply(b"\x07")
    assert e.value.status == 7 and "unbound" in str(e.value)
    with pytest.raises(AuthError):
        hs.handle_step1_reply(b"\x01" + bytes(20))


def test_strips_reply_prefix():
    assert auth.strip_reply(4, bytes([0x10, 4, 1, 9])) == bytes([1, 9])
    assert auth.strip_reply(4, bytes([5, 1, 9])) is None


def test_fixed_vectors():
    """Phone key = OpenSSL key A, strap key = OpenSSL key B."""
    hs = AuthHandshake(AUTH_KEY, crypto.KeyPair(A_PRIV))
    assert hs.step1_request == bytes.fromhex("04020002") + sec1_to_wire(A_PUB)
    assert hs.step1_request[4:28].hex() == "6ea1b44321192486099f6e4bfec7a59c5af8b5e605000000"
    reply1 = b"\x01" + CHALLENGE + sec1_to_wire(B_PUB)
    step2 = hs.handle_step1_reply(reply1)
    assert step2.hex() == "0573cad18828da0b7d861316b8773a900ecdbf1d76b11cb4c78c330850744e9dc7"
    c = hs.handle_step2_reply(b"\x01")
    assert c.session_key.hex() == "a8e497eab8f00f83868c34f9689e4a77"
    assert c.tx_counter == 0xE24FB958 and c.rx_counter == 0xF60E68B3


def test_parse_auth_key_never_echoes_input():
    assert auth.parse_auth_key("0x" + AUTH_KEY.hex()) == AUTH_KEY
    assert auth.parse_auth_key(AUTH_KEY.hex().upper()) == AUTH_KEY
    secret = "zz" + AUTH_KEY.hex()[2:]
    for bad in (secret, AUTH_KEY.hex()[:30], None, ""):
        with pytest.raises(ValueError) as e:
            auth.parse_auth_key(bad)
        assert bad is None or bad == "" or bad not in str(e.value)


def test_services_and_version_codecs():
    s = auth.ServiceList.parse(bytes.fromhex("04 0400 0000 00 8200 00 2900 01 1500 00".replace(" ", "")))
    assert s.has(0x82) and s.requires_encryption(0x29) and not s.requires_encryption(0x15)
    s2 = s.apply_change(bytes.fromhex("0501 4b00 01"))
    assert s2.requires_encryption(0x4B)
    assert not s2.apply_change(bytes.fromhex("0502 4b00")).has(0x4B)
    assert s.apply_change(b"\x04\x01\x00\x00") is None
    with pytest.raises(ValueError):
        auth.ServiceList.parse(bytes.fromhex("04010000000000"))
    assert auth.ChannelVersion.parse(b"\x02\x02\x01") == auth.ChannelVersion(2, True)
    assert auth.ChannelVersion.parse(b"\x02\x01") == auth.ChannelVersion(1, None)
    assert auth.ChannelVersion.parse(b"\x03\x02") is None


# ---- session over a simulated strap ------------------------------------------------------------------


class SimulatedStrap(StrapLink):
    """Device side of doc 01: version query, services list, ECDH auth, an encrypted battery endpoint."""

    payload_mtu = 20

    def __init__(self, key: bytes, keys=None, challenge=CHALLENGE):
        self.key = key
        self.keys = keys or crypto.generate_key_pair()
        self.challenge = challenge
        self.session: ChannelSession = None  # type: ignore[assignment]
        self.pending = None
        self.cipher = None
        self.device_session = 0
        self.phone_acks = []
        self.received = []  # (endpoint, payload, last flags)
        self.last_flags = 0
        self.lose_next_encrypted = False
        self.encrypted_acks = 0
        self.auth_info_asked = False
        self.outbox = []
        self.reassembler = transport.Reassembler(lambda ep: True, lambda s, d, n: self.cipher.decrypt(s, d, n),
                                                 self._on_message)

    def _on_message(self, m):
        self.received.append((m.endpoint, m.payload, self.last_flags))
        self.respond(m.endpoint, m.payload)

    async def write_control(self, data: bytes) -> None:
        if data == b"\x01":
            self.session.feed_control(b"\x02\x02\x01")
            return
        self.last_flags = data[1]
        a = self.reassembler.accept(data)
        if a is not None:
            status = a[3]
            enc_last = self.last_flags & transport.ENCRYPTED and self.last_flags & transport.LAST
            if self.lose_next_encrypted and enc_last:
                self.lose_next_encrypted = False
                status = transport.ACK_PACKET_LOST
            if self.last_flags & transport.ENCRYPTED:
                self.encrypted_acks += 1
            self.session.feed_control(bytes([4, 0, a[2], status, a[4]]))
        self.flush()

    async def write_data(self, data: bytes) -> None:
        self.phone_acks.append(data)

    def respond(self, endpoint, payload):
        if endpoint == Endpoints.SERVICES:
            self.outbox.append((endpoint, bytes.fromhex("04 0400 0000 00 8200 00 2900 01 1500 00".replace(" ", ""))))
        elif endpoint == Endpoints.AUTH:
            if payload[0] == 0:
                self.auth_info_asked = True
                self.outbox.append((endpoint, bytes.fromhex("10000101010001")))
            elif payload[0] == 4:
                shared = crypto.shared_secret(self.keys.private_key, payload[4:52])
                self.pending = ChannelCipher(shared, self.key)
                self.outbox.append((endpoint, b"\x10\x04\x01" + self.challenge + self.keys.public_key))
            elif payload[0] == 5:
                c = self.pending
                ok = (payload[1:17] == crypto.aes_encrypt(self.key, self.challenge)
                      and payload[17:33] == crypto.aes_encrypt(c.session_key, self.challenge))
                self.outbox.append((endpoint, bytes([0x10, 5, 1 if ok else 0x26])))
                if ok:
                    self.cipher = self._mirrored(c)
        elif endpoint == Endpoints.BATTERY:
            self.outbox.append((endpoint, bytes.fromhex("04014d00")))

    def _mirrored(self, phone: ChannelCipher) -> ChannelCipher:
        s = bytearray(24)
        s[0:4] = phone.rx_counter.to_bytes(4, "little")
        s[4:8] = phone.tx_counter.to_bytes(4, "little")
        for i in range(16):
            s[8 + i] = phone.session_key[i] ^ self.key[i]
        return ChannelCipher(bytes(s), self.key)

    def ping(self):
        self.outbox.append((Endpoints.CONNECT, b"\x03"))
        self.flush()

    def flush(self):
        while self.outbox:
            endpoint, payload = self.outbox.pop(0)
            sid = self.device_session & 0xFF
            self.device_session += 1
            enc = self.cipher is not None and endpoint == Endpoints.BATTERY
            body = self.cipher.encrypt(sid, payload) if enc else payload
            for ch in transport.build_chunks(endpoint, sid, body, len(payload), enc, self.payload_mtu):
                self.session.feed_data(ch)


def run(coro):
    return asyncio.run(asyncio.wait_for(coro, 10))


async def _setup(key=AUTH_KEY, **kw):
    strap = SimulatedStrap(key, **kw)
    logs = []
    session = ChannelSession(strap, logs.append)
    strap.session = session
    session.start()
    return strap, session, logs


def test_full_handshake_then_encrypted_request():
    async def go():
        strap, session, _ = await _setup()
        await auth.authenticate(session, AUTH_KEY)
        assert session.is_authenticated and strap.auth_info_asked
        assert session.services.requires_encryption(Endpoints.BATTERY)
        reply = await session.request(Endpoints.BATTERY, b"\x03", matches=lambda b: b[0] == 4)
        assert reply[2] == 77
        assert strap.phone_acks  # the phone ACKed multi-chunk replies on 0017
        await session.stop()
    run(go())


def test_fixed_keys_through_the_session():
    async def go():
        strap, session, _ = await _setup(keys=crypto.KeyPair(B_PRIV))
        c = await auth.authenticate(session, AUTH_KEY, key_pair=crypto.KeyPair(A_PRIV))
        assert c.session_key.hex() == "a8e497eab8f00f83868c34f9689e4a77"
        step2 = [p for ep, p, _ in strap.received if ep == Endpoints.AUTH and p[0] == 5][0]
        assert step2.hex() == "0573cad18828da0b7d861316b8773a900ecdbf1d76b11cb4c78c330850744e9dc7"
        await session.stop()
    run(go())


def test_encrypted_messages_ask_for_and_get_an_ack():
    async def go():
        strap, session, logs = await _setup()
        await auth.authenticate(session, AUTH_KEY)
        await session.send(Endpoints.BATTERY, b"\x03", want_reply=False)
        ep, _, flags = [r for r in strap.received if r[0] == Endpoints.BATTERY][0]
        assert flags & transport.NEEDS_ACK and strap.encrypted_acks == 1
        assert not any(s.startswith("no ACK") for s in logs)
        await session.stop()
    run(go())


def test_encrypted_message_reported_lost_is_sent_again():
    async def go():
        strap, session, _ = await _setup()
        await auth.authenticate(session, AUTH_KEY)
        strap.lose_next_encrypted = True
        await session.send(Endpoints.BATTERY, b"\x03")
        assert sum(1 for r in strap.received if r[0] == Endpoints.BATTERY) == 2
        await session.stop()
    run(go())


def test_answers_ping_with_pong_without_ack_request():
    async def go():
        strap, session, _ = await _setup()
        await auth.authenticate(session, AUTH_KEY)
        strap.ping()
        for _ in range(50):
            await asyncio.sleep(0.01)
            pongs = [r for r in strap.received if r[0] == Endpoints.CONNECT]
            if pongs:
                break
        assert pongs[0][1] == b"\x04"
        assert pongs[0][2] == transport.FIRST | transport.LAST
        assert session.pings == 1
        await session.stop()
    run(go())


def test_wrong_key_fails_with_status():
    async def go():
        strap, session, _ = await _setup()
        with pytest.raises(AuthError) as e:
            await auth.authenticate(session, bytes(16))
        assert e.value.status == 0x26
        assert not session.is_authenticated
        await session.stop()
    run(go())


def test_auth_payloads_are_not_logged():
    async def go():
        strap, session, logs = await _setup()
        await auth.authenticate(session, AUTH_KEY)
        text = "\n".join(logs)
        assert AUTH_KEY.hex() not in text
        assert session.cipher.session_key.hex() not in text
        assert "> 0082" not in text
        await session.stop()
    run(go())
