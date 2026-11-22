# Amazfit Helio Strap: hardware and data map

Verified on our strap from a Mac (bleak): firmware 3.27.0.1, hardware rev 0.132.27.2,
advertises 180D + FEE0, manufacturer 0x0157. Live HR on 2A37 and battery on 2A19 work
with no key. Everything else needs the strap's 16-byte auth key.

Sources: github.com/sayed0am/unstrap (docs/protocol, protocol/ Kotlin module),
Gadgetbridge zeppos code on Codeberg.

## Data streams

All history uses GDSP: control on endpoint 0x004B, bulk data on FEE0/0005.

| Stream | Measures | Granularity | Endpoint / GDSP type | Key needed |
|---|---|---|---|---|
| HR live | bpm | ~1 Hz | 2A37; also 0x001D + 0x0048 | no (2A37) |
| HR per second | bpm | 1 s, zlib | GDSP 87 | yes |
| HR per minute | bpm | 1 min | GDSP 1 byte 3 | yes |
| Resting / max HR | bpm | daily | GDSP 58 / 61 | yes |
| HRV | RMSSD, SDNN | per night | GDSP 73 / 78 | yes |
| SpO2 | % | spot + sleep | GDSP 37 / 38 / 39 | yes |
| Stress | 0-100 | 1 min | GDSP 19 / 18 | yes |
| Sleep stages | light/deep/awake/REM + score | per day | GDSP 72 | yes |
| Sleep respiratory rate | breaths/min | per sample | GDSP 56 | yes |
| Skin temperature | 0.01 C | 1 min | GDSP 46 (Zepp never fetches it) | yes |
| Steps / activity | mode, intensity, steps, HR | 1 min; strap keeps ~43 h | GDSP 1 / 88 | yes |
| Steps live | steps, m, kcal | ~1 s | 0x0016 `05 01` | yes |
| PAI | daily, total, zones | daily | 0x0022 `0C` or GDSP 13 | yes |
| Readiness | score + HRV, RHR, skin, breathing sub-scores | daily | 0x0049 `08 <since>` | yes |
| Workouts | summary + detail | per workout | GDSP 5, 6, 59, 70 | yes |
| Health alerts | high/low HR, stress, SpO2 | event | 0x001D `0D` | yes |
| Raw accelerometer | x/y/z | live | FEE0/0001 + 0002 | not confirmed |

## Controls

- Vibration pattern: 0x0018 `03 scene 01 01 n {u16 ms}*` (encrypted).
- Find device: 0x001A `03` start, `06` stop.
- Alarms: 0x000F (add 03, delete 05, modify 07, list 09, clear 0B).
- Settings: 0x000A group 0x08 key 0x01 = HR interval (0xFE continuous); 0x13 stress; 0x31 SpO2.

## Auth (bound strap)

1. Key: 16 bytes (32 hex) from the Zepp account (huami-token or the account device list).
2. Subscribe 0016/0017, write `01`, expect `02 02 <enc>`.
3. Services list: `03` to endpoint 0x0000.
4. ECDH on NIST B-163: send `04 02 00 02` + pub48 to 0x0082.
5. sessionKey[i] = S[8+i] XOR K[i]; counters from S[0..7].
6. Send `05` + AES-ECB(K, rnd) + AES-ECB(sessionKey, rnd); status 1 = ok.
7. Encrypted frames: payload || u32 counter || CRC32, AES-ECB, chunk flag 0x08.

Port order for a Python client: transport, crypto (B163, ChannelCrypto), auth, history
(GdspClient, FetchCodec, RecordParsers), live (Services), controls (Settings, Alarms, FindDevice).
