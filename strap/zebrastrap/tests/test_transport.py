"""ChunksTest.kt."""

from zebrastrap import transport as t
from zebrastrap.crypto import ChannelCipher, encrypted_size


def test_single_chunk_matches_spec_example():
    chunks = t.build_chunks(endpoint=0, session_id=0, data=b"\x03", plain_length=1, encrypted=False, payload_mtu=20)
    assert chunks == [bytes.fromhex("030700000001000000000003")]


def test_multi_chunk_split_matches_spec_table():
    data = bytes(range(52))
    chunks = t.build_chunks(0x0082, 7, data, 52, encrypted=False, payload_mtu=20)
    assert [len(c) for c in chunks] == [20, 20, 20, 18]
    assert [c[1] for c in chunks] == [0x01, 0x00, 0x00, 0x06]
    assert [c[4] for c in chunks] == [0, 1, 2, 3]
    assert chunks[0][5] == 0x34
    assert chunks[0][9] == 0x82


def test_every_16th_chunk_asks_for_ack_and_no_reply_flag():
    chunks = t.build_chunks(1, 0, bytes(400), 400, False, 20, want_reply=False)
    asks = [i for i, c in enumerate(chunks) if c[1] & t.NEEDS_ACK]
    assert asks == [15]
    assert chunks[-1][1] == t.LAST


def test_reassembles_and_acks():
    data = bytes((i * 3) & 0xFF for i in range(40))
    got = []
    r = t.Reassembler(lambda ep: True, lambda s, c, n: c, got.append)
    chunks = t.build_chunks(0x0029, 5, data, 40, encrypted=False, payload_mtu=20)
    acks = [r.accept(c) for c in chunks]
    assert len(got) == 1 and got[0].payload == data and got[0].endpoint == 0x0029
    assert acks[0] is None
    assert len(chunks) == 4  # 9 + 15 + 15 + 1
    assert acks[-1] == bytes.fromhex("0400050103")


def test_detects_lost_chunk():
    r = t.Reassembler(lambda ep: True, lambda s, c, n: c, lambda m: None)
    chunks = t.build_chunks(1, 9, bytes(40), 40, encrypted=False, payload_mtu=20)
    r.accept(chunks[0])
    assert r.accept(chunks[2])[3] == t.ACK_PACKET_LOST


def test_lost_chunk_drops_the_message_and_later_chunks_are_ignored():
    got = []
    r = t.Reassembler(lambda ep: True, lambda s, c, n: c, got.append)
    chunks = t.build_chunks(1, 9, bytes(40), 40, encrypted=False, payload_mtu=20)
    r.accept(chunks[0])
    r.accept(chunks[2])
    assert r.accept(chunks[3]) is None
    assert not got
    for c in t.build_chunks(1, 10, bytes(5), 5, encrypted=False, payload_mtu=20):
        r.accept(c)
    assert len(got) == 1


def test_undecryptable_message_is_acked_and_dropped():
    got, failures = [], []

    def bad(s, c, n):
        raise ValueError("bad crc")

    r = t.Reassembler(lambda ep: True, bad, got.append)
    r.on_decrypt_failure = failures.append
    (chunk,) = t.build_chunks(1, 3, bytes(16), 4, encrypted=True, payload_mtu=40)
    assert r.accept(chunk)[3] == t.ACK_SUCCESS
    assert not got and len(failures) == 1


def test_rejects_unknown_endpoint():
    r = t.Reassembler(lambda ep: ep == 1, lambda s, c, n: c, lambda m: None)
    assert r.accept(t.build_chunks(2, 1, b"\x01", 1, False, 20)[0])[3] == t.ACK_UNKNOWN_ENDPOINT


def test_bad_length_and_foreign_app_id():
    r = t.Reassembler(lambda ep: True, lambda s, c, n: c, lambda m: None)
    first = bytearray(t.build_chunks(1, 1, bytes(5), 5, False, 20)[0])
    first[5] = 3  # totalLength 3 but 5 bytes follow
    assert r.accept(bytes(first))[3] == t.ACK_BAD_FORMAT
    other = bytearray(t.build_chunks(1, 1, bytes(5), 5, False, 20)[0])
    other[2] = 1
    assert r.accept(bytes(other)) is None


def test_encrypted_round_trip_through_chunks_and_cipher():
    shared = bytes(i + 1 for i in range(24))
    key = bytes(0xA0 + i for i in range(16))
    phone = ChannelCipher(shared, key)
    payload = b"hello strap, this is a longer encrypted message"
    ct = phone.encrypt(3, payload)
    assert len(ct) == encrypted_size(len(payload))
    s = bytearray(shared)
    s[4:8] = s[0:4]  # the device's rx counter equals our initial tx counter
    device = ChannelCipher(bytes(s), key)
    got = []
    r = t.Reassembler(lambda ep: True, device.decrypt, got.append)
    for c in t.build_chunks(0x0047, 3, ct, len(payload), encrypted=True, payload_mtu=20):
        assert c[1] & t.ENCRYPTED
        r.accept(c)
    assert got[0].payload == payload and got[0].encrypted


def test_parses_both_ack_forms():
    assert t.parse_device_ack(bytes.fromhex("0400010103")) == t.DeviceAck(1, 1, 3)
    assert t.parse_device_ack(bytes.fromhex("04050102")) == t.DeviceAck(5, 1, 2)
    assert t.parse_device_ack(bytes.fromhex("0405010200")) is None
    assert t.parse_device_ack(bytes.fromhex("03")) is None
