#!/usr/bin/env python3
"""Write GPS coordinates to JPEG EXIF metadata.

Usage: write_exif_gps.py <jpeg_path> <lat> <lng>

Tries exiftool first, then raw binary fallback.
"""
import json
import os
import struct
import subprocess
import sys
from typing import Optional


def exif_bytes_to_rationals(exif_data: bytes, offset: int, count: int) -> list[float]:
    vals = []
    for i in range(count):
        off = offset + i * 8
        num = struct.unpack('<I', exif_data[off:off+4])[0]
        den = struct.unpack('<I', exif_data[off+4:off+8])[0]
        vals.append(num / den if den != 0 else 0.0)
    return vals


def deg_to_rationals(dec: float) -> list[tuple[int, int, int, int, int, int]]:
    sign = -1 if dec < 0 else 1
    dec = abs(dec)
    d = int(dec)
    m = int((dec - d) * 60)
    s = (dec - d - m / 60) * 3600
    s_int = int(s)
    s_frac = int(round((s - s_int) * 10000000))
    last_den = 10000000
    while last_den > 1 and s_frac % 10 == 0:
        s_frac //= 10
        last_den //= 10
    return [
        (d, 1),
        (m, 1),
        (s_int * last_den + s_frac, last_den),
    ]


def rationals_to_bytes(rationals: list[tuple[int, int]]) -> bytes:
    data = b''
    for num, den in rationals:
        data += struct.pack('<II', num, den)
    return data


def write_gps_exif_exiftool(path: str, lat: float, lng: float) -> bool:
    try:
        result = subprocess.run(
            ['exiftool', '-overwrite_original',
             f'-GPSLatitudeRef={ "N" if lat >= 0 else "S" }',
             f'-GPSLatitude={ abs(lat) }',
             f'-GPSLongitudeRef={ "E" if lng >= 0 else "W" }',
             f'-GPSLongitude={ abs(lng) }',
             path],
            capture_output=True, timeout=30
        )
        return result.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return False


def write_gps_exif_raw(path: str, lat: float, lng: float) -> bool:
    with open(path, 'rb') as f:
        data = bytearray(f.read())

    if data[:2] != b'\xff\xd8':
        return False

    idx = data.find(b'\xff\xe1')
    if idx < 0 or not (data[idx+2:idx+4] and len(data) > idx + 4):
        return False

    app1_len = struct.unpack('>H', data[idx+2:idx+4])[0]
    if data[idx+4:idx+10] != b'Exif\0\0':
        return False

    tiff_start = idx + 10
    endian = '<' if data[tiff_start:tiff_start+2] == b'II' else '>'

    ifd0_off = struct.unpack(endian + 'I', data[tiff_start+4:tiff_start+8])[0]
    ifd0_abs = tiff_start + ifd0_off

    num_entries = struct.unpack(endian + 'H', data[ifd0_abs:ifd0_abs+2])[0]
    entries_end = ifd0_abs + 2 + num_entries * 12

    gps_ifd_offset_entry = None
    for i in range(num_entries):
        entry_off = ifd0_abs + 2 + i * 12
        tag = struct.unpack(endian + 'H', data[entry_off:entry_off+2])[0]
        if tag == 0x8825:
            gps_ifd_offset_entry = entry_off
            break

    lat_ref = 'N' if lat >= 0 else 'S'
    lng_ref = 'E' if lng >= 0 else 'W'
    lat_rat = deg_to_rationals(lat)
    lng_rat = deg_to_rationals(lng)

    gps_ifd_data = b''
    entries: list[tuple[int, int, int, bytes]] = []

    entries.append((0x0000, 1, 4, struct.pack('BBBB', 2, 3, 0, 0)))
    entries.append((0x0001, 2, 2, lat_ref.encode() + b'\0'))
    entries.append((0x0003, 2, 2, lng_ref.encode() + b'\0'))

    lat_bytes = rationals_to_bytes(lat_rat)
    lng_bytes = rationals_to_bytes(lng_rat)

    entries.append((0x0002, 5, 3, lat_bytes))
    entries.append((0x0004, 5, 3, lng_bytes))

    gps_ifd_start = len(data)
    num_gps = len(entries)
    gps_ifd_body = struct.pack(endian + 'H', num_gps)

    data_blocks = b''
    entry_data_offset = gps_ifd_start + 2 + num_gps * 12 + 4

    for tag, typ, count, val in entries:
        if len(val) <= 4:
            padded = val + b'\0' * (4 - len(val))
            gps_ifd_body += struct.pack(endian + 'HHII', tag, typ, count,
                                       struct.unpack(endian + 'I', padded)[0])
        else:
            gps_ifd_body += struct.pack(endian + 'HHII', tag, typ, count,
                                       entry_data_offset + len(data_blocks))
            data_blocks += val

    gps_ifd_body += struct.pack(endian + 'I', 0)
    gps_ifd_data = gps_ifd_body + data_blocks

    new_ifd0: Optional[bytes] = None

    if gps_ifd_offset_entry is not None:
        gps_ifd_offset_entry += entries_end - ifd0_abs

    data.extend(b'\0' * len(gps_ifd_data))
    data[gps_ifd_start:gps_ifd_start + len(gps_ifd_data)] = gps_ifd_data

    if gps_ifd_offset_entry is not None:
        struct.pack_into(endian + 'I', data, gps_ifd_offset_entry + 8,
                        gps_ifd_start - tiff_start)
    else:
        entries_type_4 = 4
        gps_tag_value = gps_ifd_start - tiff_start
        extra_entry = struct.pack(endian + 'HHII', 0x8825, entries_type_4, 1, gps_tag_value)
        insert_pos = entries_end
        data[insert_pos:insert_pos] = extra_entry
        num_entries += 1
        struct.pack_into(endian + 'H', data, ifd0_abs, num_entries)
        app1_new_len = len(data) - idx - 2
        if app1_new_len > 65535:
            return False
        struct.pack_into('>H', data, idx + 2, app1_new_len)

    with open(path, 'wb') as f:
        f.write(data)
    return True


def main():
    if len(sys.argv) != 4:
        print(json.dumps({"success": False, "error": "Usage: write_exif_gps.py <path> <lat> <lng>"}))
        sys.exit(1)

    path = sys.argv[1]
    try:
        lat = float(sys.argv[2])
        lng = float(sys.argv[3])
    except ValueError as e:
        print(json.dumps({"success": False, "error": str(e)}))
        sys.exit(1)

    if write_gps_exif_exiftool(path, lat, lng):
        print(json.dumps({"success": True, "method": "exiftool"}))
    elif write_gps_exif_raw(path, lat, lng):
        print(json.dumps({"success": True, "method": "raw"}))
    else:
        print(json.dumps({"success": False, "error": "Failed to write EXIF GPS"}))


if __name__ == '__main__':
    main()
