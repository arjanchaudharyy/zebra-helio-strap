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

## Strap settings, full dump (read 2026-11-23)

Read with the settings query `03 01 <group> 00` on every group 0x00 to 0x13. Groups 0x00, 0x08, 0x09, 0x0A and 0x0B answer. The rest do not.
"Known" means confirmed from protocol code or by behaviour. "Inferred" means a guess from the value and its choices.

| Group | Key | Value | Choices | Meaning |
|---|---|---|---|---|
| 0x08 health | 0x01 | 254 | 0, 254, 1, 5, 10, 30 | Heart rate interval: 254 = continuous (known) |
| 0x08 | 0x02 | 130 | 0, 100 to 150 | High heart rate alert, bpm (known) |
| 0x08 | 0x03 | 50 | 0, 40, 45, 50 | Low heart rate alert, bpm (known) |
| 0x08 | 0x04 | 0 | on/off | Activity heart rate boost (known name) |
| 0x08 | 0x11 to 0x16 | 1 | on/off | Health toggles, all on (inferred: sleep, breathing, stress) |
| 0x08 | 0x18 | 1 | 0, 1, 5, 10, 15, 30 | A measurement interval in minutes (inferred: stress) |
| 0x08 | 0x31 / 0x32 | on / 80 | 0, 80, 85, 90 | Blood oxygen tracking, low oxygen alert at 80 % (known) |
| 0x08 | 0x41 to 0x43 | on, 480, 1260 | | Inactivity reminder 08:00 to 21:00 (inferred) |
| 0x08 | 0x44 to 0x46 | on, 720, 840 | | Reminder pause 12:00 to 14:00 (inferred) |
| 0x08 | 0x52 to 0x57 | 7000, 500, 12200, 360, 6, 36 | | Daily goals (0x53 = calorie target, known; others inferred) |
| 0x09 workout | 0x05 | 80, 100, 120, 140, 160, 180 | | Heart rate zone limits, set 1 (inferred) |
| 0x09 | 0x06 | 102, 122, 142, 163, 183, 204 | | Heart rate zone limits: exactly 50, 60, 70, 80, 90 and 100 % of max HR 204 (220 minus age 16). These are the zones the Zepp app shows (confirmed by arithmetic) |
| 0x09 | 0x07 | 60 | | Resting heart rate used for zones (inferred) |
| 0x0A general | 0x2D | 0 | | Restart trigger (known) |

### No beat-to-beat (RR) data over the live link
- The standard heart rate notification (2A37) has flag byte `00` on every packet. It carries the bpm only: no RR intervals, no skin-contact bit and no energy field.
- No settings key switches RR on.
- HRV is available only from history: RMSSD (GDSP 73) and SDNN (GDSP 78), once a night.
