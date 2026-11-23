"""FindDeviceTest.kt and the settings parts of AlarmsAndSettingsTest.kt."""

import pytest

from zebrastrap.controls import (TYPE_BOOL, TYPE_I32, FindDevice, MotorVibrate, SettingsCodec, SettingsGroup,
                                 SettingsProtocol)

hx = bytes.fromhex


def test_find_device_commands():
    assert FindDevice.BUZZ == hx("03")
    assert FindDevice.START == hx("0501")
    assert FindDevice.STOP == hx("06")
    assert FindDevice.is_buzz_ok(hx("040101"))
    assert FindDevice.is_buzz_ok(hx("0401"))
    assert not FindDevice.is_buzz_ok(hx("0400"))
    assert not FindDevice.is_buzz_ok(hx("04"))
    assert FindDevice.FIND_TIMEOUT_MS == 60_000


def test_motor_pattern():
    assert MotorVibrate.play([200, 150, 200]) == hx("0300010102c8009600c8000000")
    assert MotorVibrate.play([150, 150, 150]) == hx("03000101029600960096000000")  # doc 01 example
    assert MotorVibrate.is_ok(hx("0401"))
    assert not MotorVibrate.is_ok(hx("0400"))
    with pytest.raises(ValueError):
        MotorVibrate.play([])
    with pytest.raises(ValueError):
        MotorVibrate.play([70000])


HEALTH = hx(
    "04010803011d0110fe0600fe01050a1e0210000700646e78828c960310000400282d32040b00830b01110b01120b01130b01"
    "140b01160b00181000060001050a0f1e310b0132105a040050555a410b014230080043301500440b0145300c0046300e00510b0152"
    "0310270000d00700003075000053012c01c800b80b5403b036000058020000a08c00005501e0010000a00556010c00060010005701"
    "1e0005005a00711000020001210b00220b00")


def test_health_group():
    g = SettingsCodec.parse(HEALTH)
    assert (g.group, g.version, len(g.settings)) == (8, 3, 29)
    interval = g[SettingsCodec.KEY_HR_INTERVAL]
    assert interval.number == 0xFE and interval.choices == (0, 0xFE, 1, 5, 10, 30)
    assert g[SettingsCodec.KEY_HIGH_HR_ALERT].choices == (0, 100, 110, 120, 130, 140, 150)
    assert g[SettingsCodec.KEY_HIGH_HR_ALERT].number == 0
    assert g[SettingsCodec.KEY_LOW_HR_ALERT].choices == (0, 40, 45, 50)
    assert g[SettingsCodec.KEY_ACTIVITY_HR].bool is False
    assert g[SettingsCodec.KEY_STRESS].bool is True
    assert g[SettingsCodec.KEY_LOW_SPO2_ALERT].number == 90
    assert g[0x42].number == 8 * 60
    assert g[0x52].number == 10_000


def test_hr_interval_write():
    g = SettingsCodec.parse(HEALTH)
    assert SettingsCodec.hr_interval_write(g, 10) == hx("050803000101100a")
    assert SettingsCodec.hr_interval_write(g, SettingsCodec.HR_CONTINUOUS) == hx("05080300010110fe")
    assert SettingsCodec.hr_interval_write(g, 0) == hx("0508030001011000")
    with pytest.raises(ValueError):
        SettingsCodec.hr_interval_write(g, 7)  # not one of the strap's choices
    with pytest.raises(ValueError):
        SettingsCodec.hr_interval_write(SettingsGroup(10, 2, {}), 10)


def test_calorie_target_and_int_settings():
    g = SettingsCodec.parse(HEALTH)
    assert g[SettingsCodec.KEY_CALORIE_TARGET].number == 300
    s = SettingsCodec.int_setting(SettingsCodec.KEY_CALORIE_TARGET, g[SettingsCodec.KEY_CALORIE_TARGET].type, 450)
    assert SettingsCodec.write(8, 3, [s]) == hx("05080300015301c201")
    assert SettingsCodec.int_setting(0x52, TYPE_I32, 1000).raw == hx("e8030000")
    assert SettingsCodec.int_setting(0x52, TYPE_BOOL, 1) is None


def test_settings_write_and_query():
    req = SettingsCodec.write(8, 3, [SettingsCodec.enum_setting(SettingsCodec.KEY_HIGH_HR_ALERT, 120)])
    assert req == hx("0508030001021078")
    assert SettingsCodec.is_write_success(hx("0601"))
    assert not SettingsCodec.is_write_success(hx("0604"))
    assert SettingsCodec.query(8, [2, 3]) == hx("030108020203")
    assert SettingsCodec.query(SettingsCodec.GROUP_DEVICE_INFORMATION, with_options=False) == hx("03000000")
    # keys go out ascending
    two = SettingsCodec.write(8, 3, [SettingsCodec.bool_setting(0x13, True), SettingsCodec.enum_setting(1, 5)])
    assert two == hx("0508030002011005130b01")


def test_restart_style_bool_write_and_parse():
    assert SettingsCodec.write(SettingsCodec.GROUP_GENERAL, 2,
                               [SettingsCodec.bool_setting(SettingsCodec.KEY_RESTART, True)]) == hx("050a0200012d0b01")
    g = SettingsCodec.parse(hx("04010a0200012d0b00"))
    assert g[SettingsCodec.KEY_RESTART].bool is False


def test_settings_version_and_flag_bytes():
    assert SettingsCodec.version_request == hx("01")
    assert SettingsCodec.parse_version(hx("0203")) == SettingsProtocol(3, ())
    assert SettingsCodec.parse_version(hx("020402080a0100")) == SettingsProtocol(4, (8, 10))
    assert SettingsCodec.parse_version(hx("0402")) is None
    assert [SettingsCodec.flag_bytes(v) for v in (None, 2, 3, 4, 5, 6, 9)] == [0, 0, 0, 1, 2, 3, 3]


def test_flag_bytes_from_protocol_4_and_6():
    v4 = hx("04010803010201100a02000a00130b0101")
    g = SettingsCodec.parse(v4, protocol_version=4)
    assert g[SettingsCodec.KEY_HR_INTERVAL].number == 10
    assert g[SettingsCodec.KEY_HR_INTERVAL].choices == (0, 10)
    assert g[SettingsCodec.KEY_HR_INTERVAL].flags == (0,)
    assert g[SettingsCodec.KEY_STRESS].bool is True and g[SettingsCodec.KEY_STRESS].flags == (1,)
    g_no_flags = SettingsCodec.parse(v4)
    assert g_no_flags is None or g_no_flags[SettingsCodec.KEY_STRESS] is None
    v6 = hx("040108030002010b01000001130b00010000")
    g6 = SettingsCodec.parse(v6, protocol_version=6)
    assert g6[SettingsCodec.KEY_STRESS].bool is False and g6[SettingsCodec.KEY_STRESS].flags == (1, 0, 0)


def test_failed_query_and_reply_matching():
    assert SettingsCodec.parse(hx("0403")) is None
    assert SettingsCodec.reply_code(hx("0403")) == 3
    assert SettingsCodec.reply_group(HEALTH) == 8
    assert SettingsCodec.is_reply_to(SettingsCodec.GROUP_HEALTH, HEALTH)
    general = hx("04010a0200012f0b00")
    assert not SettingsCodec.is_reply_to(SettingsCodec.GROUP_HEALTH, general)
    assert SettingsCodec.is_reply_to(SettingsCodec.GROUP_GENERAL, general)
    assert SettingsCodec.is_reply_to(SettingsCodec.GROUP_HEALTH, hx("0403"))
    assert not SettingsCodec.is_reply_to(SettingsCodec.GROUP_HEALTH, hx("0601"))


def test_push_updates_only_its_keys():
    read = SettingsCodec.parse(HEALTH)
    merged = read.updated_with(SettingsCodec.parse(hx("040108040001021078")))
    assert merged.version == 4 and merged[SettingsCodec.KEY_HIGH_HR_ALERT].number == 120
    assert len(merged.settings) == 29
    assert merged[SettingsCodec.KEY_HR_INTERVAL] == read[SettingsCodec.KEY_HR_INTERVAL]
    assert read.updated_with(SettingsCodec.parse(hx("04010a0200012f0b00"))) is None


def test_device_information_strings():
    reply = hx(("0401000100 05" "02500f000000" "0320332c3100" "042031322c363500" "052062616400"
                "0620322c332c3900").replace(" ", ""))
    g = SettingsCodec.parse(reply)
    assert g[2].number == 15 and g[4].text == "12,65" and g[2].text is None
