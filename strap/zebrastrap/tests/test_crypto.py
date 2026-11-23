"""B163Test.kt and the channel cipher parts of ChunksTest.kt, plus OpenSSL vectors.

`cryptography` has no sect163r2, so the BouncyCastle comparisons are replaced by vectors made with
OpenSSL 4.1 (`openssl ecparam -name sect163r2 -genkey`, `openssl pkeyutl -derive`).
"""

import secrets
import zlib

import pytest
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

from zebrastrap import crypto
from zebrastrap.crypto import B, G, GX, GY, N, ChannelCipher, DecryptError

# Two OpenSSL key pairs on sect163r2. Public keys are SEC1 uncompressed (04 || X || Y), big-endian.
A_PRIV = 0x02274E34838C799A4994CC450DE2726820EE18EF1F
A_PUB = bytes.fromhex("0405e6b5f85a9ca5c7fe4b6e9f098624192143b4a16e"
                      "06927229fe53778551714a8e101fcbe62e5313acdf")
B_PRIV = 0x009FC68AB9A5E12C8CDF25F1104F39E5455B4EEE9C
B_PUB = bytes.fromhex("0404911d2a27352f4837ffa9b2b8d75bdc7c6d01415f"
                      "005d4b65f646fa8c6f0b9e4018372dcc9c907c4844")
AB_SHARED_X = bytes.fromhex("05e2c4492e642ded23e4f8ae94f60e68b3e24fb958")  # big-endian X


def sec1_to_wire(pub: bytes) -> bytes:
    x, y = pub[1:22], pub[22:43]
    return x[::-1] + b"\0" * 3 + y[::-1] + b"\0" * 3


def test_curve_constants_are_nist_b163():
    # FIPS 186-4 D.1.3.2.2 (B-163).
    assert B == int("20a601907b8c953ca1481eb10512f78744a3205fd", 16)
    assert GX == int("3f0eba16286a2d57ea0991168d4994637e8343e36", 16)
    assert GY == int("0d51fbc6c71a0094fa2cdd545b11c5c0c797324f1", 16)
    assert N == int("40000000000000000000292fe77e70c12a4234c33", 16)


def test_generator_is_on_curve_and_has_order_n():
    assert crypto.is_on_curve(G)
    assert crypto.multiply(N, G).is_infinity
    assert crypto.multiply(1, G) == G
    assert crypto.multiply(N - 1, G) == crypto.Point(GX, GX ^ GY)  # -P = (x, x + y)


def test_field_inverse():
    a = int("123456789abcdef0123456789abcdef01234567", 16)
    assert crypto.gf_mul(a, crypto.gf_inv(a)) == 1
    with pytest.raises(ValueError):
        crypto.gf_inv(0)


def test_public_keys_match_openssl():
    assert crypto.KeyPair(A_PRIV).public_key == sec1_to_wire(A_PUB)
    assert crypto.KeyPair(B_PRIV).public_key == sec1_to_wire(B_PUB)


def test_shared_secret_matches_openssl():
    s1 = crypto.shared_secret(A_PRIV, sec1_to_wire(B_PUB))
    s2 = crypto.shared_secret(B_PRIV, sec1_to_wire(A_PUB))
    assert s1 == s2
    assert len(s1) == 24
    assert s1 == AB_SHARED_X[::-1] + b"\0" * 3


def test_ecdh_is_symmetric_and_uses_little_endian():
    a = crypto.generate_key_pair()
    b = crypto.generate_key_pair()
    assert len(a.public_key) == 48
    assert crypto.shared_secret(a.private_key, b.public_key) == crypto.shared_secret(b.private_key, a.public_key)
    g = crypto.encode_point(G)
    assert g[0] == 0x36 and g[23] == 0x00  # Gx's lowest byte first


def test_scalar_multiplication_is_linear():
    for _ in range(3):
        k1, k2 = secrets.randbits(160), secrets.randbits(160)
        assert crypto.point_add(crypto.multiply(k1, G), crypto.multiply(k2, G)) == crypto.multiply(k1 + k2, G)


def test_decode_point_rejects_off_curve():
    bad = bytearray(crypto.encode_point(G))
    bad[30] ^= 1
    with pytest.raises(ValueError):
        crypto.decode_point(bytes(bad))
    with pytest.raises(ValueError):
        crypto.decode_point(b"\0" * 47)


# ---- AES and channel cipher -------------------------------------------------------------------

def test_aes_ecb_fips197_vector():
    key = bytes.fromhex("000102030405060708090a0b0c0d0e0f")
    pt = bytes.fromhex("00112233445566778899aabbccddeeff")
    ct = bytes.fromhex("69c4e0d86a7b0430d8cdb78070b4c55a")
    assert crypto.aes_encrypt(key, pt) == ct
    assert crypto.aes_decrypt(key, ct) == pt
    with pytest.raises(ValueError):
        crypto.aes_encrypt(key, b"short")


SHARED = bytes(range(1, 25))
KEY = bytes(0xA0 + i for i in range(16))


def test_session_key_and_counters():
    c = ChannelCipher(SHARED, KEY)
    assert c.session_key == bytes(SHARED[8 + i] ^ KEY[i] for i in range(16))
    assert c.session_key.hex() == "a9aba9afa9aba9b7b9bbb9bfb9bbb9b7"
    assert c.tx_counter == 0x04030201
    assert c.rx_counter == 0x08070605


def _reference_encrypt(session_key: bytes, sid: int, counter: int, payload: bytes) -> bytes:
    """Independent build of doc 01 section 3.6: payload || u32 cnt || u32 crc32, zero-padded, AES-ECB."""
    body = payload + counter.to_bytes(4, "little")
    plain = body + (zlib.crc32(body) & 0xFFFFFFFF).to_bytes(4, "little")
    plain += b"\0" * (-len(plain) % 16)
    key = bytes(b ^ sid for b in session_key)
    e = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
    return e.update(plain) + e.finalize()


def test_encrypt_matches_reference_format_and_advances_counter():
    c = ChannelCipher(SHARED, KEY)
    payload = b"\x03"
    ct = c.encrypt(3, payload)
    assert ct == _reference_encrypt(c.session_key, 3, 0x04030201, payload)
    assert len(ct) == 16
    assert c.tx_counter == 0x04030202
    ct2 = c.encrypt(4, b"x" * 9)  # 9 + 8 = 17 -> 32 bytes
    assert len(ct2) == 32 == crypto.encrypted_size(9)


def _device_view(phone: ChannelCipher) -> ChannelCipher:
    """A cipher whose rx counter is the phone's tx counter and whose session key is the same."""
    s = bytearray(24)
    s[0:4] = phone.rx_counter.to_bytes(4, "little")
    s[4:8] = phone.tx_counter.to_bytes(4, "little")
    for i in range(16):
        s[8 + i] = phone.session_key[i] ^ KEY[i]
    return ChannelCipher(bytes(s), KEY)


def test_decrypt_round_trip_crc_and_replay():
    phone = ChannelCipher(SHARED, KEY)
    dev = _device_view(phone)
    msg = b"hello strap, this is a longer encrypted message"
    ct = phone.encrypt(7, msg)
    assert dev.decrypt(7, ct, len(msg)) == msg
    assert dev.rx_counter == 0x04030201
    # wrong session id -> wrong key -> CRC mismatch
    with pytest.raises(DecryptError):
        _device_view(ChannelCipher(SHARED, KEY)).decrypt(8, ct, len(msg))
    with pytest.raises(DecryptError):
        dev.decrypt(7, ct[:-16], len(msg))
    # a counter far behind the window is rejected
    old = ChannelCipher(SHARED, KEY)
    old.tx_counter = (dev.rx_counter - 200_000) % 0xFFFFFFFF
    with pytest.raises(DecryptError, match="replay"):
        dev.decrypt(1, old.encrypt(1, b"a"), 1)


def test_counter_wraps_modulo_ffffffff():
    c = ChannelCipher(b"\xfe\xff\xff\xff" + bytes(20), KEY)
    assert c.tx_counter == 0xFFFFFFFE
    c.encrypt(0, b"a")
    assert c.tx_counter == 0  # (0xFFFFFFFE + 1) % 0xFFFFFFFF
