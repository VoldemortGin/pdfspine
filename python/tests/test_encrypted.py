"""`PYFITZ-002` — encrypted-PDF read flow from Python (PRD §8.4).

Builds a self-generated RC4 (V2/R3, 128-bit) encrypted PDF entirely in pure
Python (RC4 + the Standard Security Handler R3 key derivation), then drives the
``needs_pass`` → ``authenticate`` → load-pages flow through both ``oxipdf`` and
``fitz``. No external files (PRD §10).
"""

from __future__ import annotations

import hashlib
import struct

import oxipdf
import pytest

# The 32-byte padding string from ISO 32000-1 §7.6.3.3 (Algorithm 2).
_PAD = bytes(
    [
        0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41,
        0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
        0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80,
        0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
    ]
)


def _rc4(key: bytes, data: bytes) -> bytes:
    s = list(range(256))
    j = 0
    for i in range(256):
        j = (j + s[i] + key[i % len(key)]) & 0xFF
        s[i], s[j] = s[j], s[i]
    out = bytearray()
    i = j = 0
    for byte in data:
        i = (i + 1) & 0xFF
        j = (j + s[i]) & 0xFF
        s[i], s[j] = s[j], s[i]
        out.append(byte ^ s[(s[i] + s[j]) & 0xFF])
    return bytes(out)


def _pad_pw(pw: bytes) -> bytes:
    return (pw + _PAD)[:32]


def _compute_o(owner_pw: bytes, user_pw: bytes, key_len: int) -> bytes:
    # Algorithm 3 (R3): MD5 x51 of padded owner pw → RC4 x20 over padded user pw.
    h = hashlib.md5(_pad_pw(owner_pw)).digest()
    for _ in range(50):
        h = hashlib.md5(h[:key_len]).digest()
    rc4_key = h[:key_len]
    o = _pad_pw(user_pw)
    for i in range(20):
        k = bytes(b ^ i for b in rc4_key)
        o = _rc4(k, o)
    return o


def _file_key(user_pw: bytes, o: bytes, p: int, id0: bytes, key_len: int) -> bytes:
    # Algorithm 2 (R3): MD5 over padded pw + O + P(LE) + ID[0], then x50 MD5.
    md = hashlib.md5()
    md.update(_pad_pw(user_pw))
    md.update(o)
    md.update(struct.pack("<i", p))
    md.update(id0)
    h = md.digest()
    for _ in range(50):
        h = hashlib.md5(h[:key_len]).digest()
    return h[:key_len]


def _compute_u(file_key: bytes, id0: bytes) -> bytes:
    # Algorithm 5 (R3): MD5 of pad+ID[0] → RC4 x20 → pad to 32.
    md = hashlib.md5()
    md.update(_PAD)
    md.update(id0)
    h = md.digest()
    for i in range(20):
        k = bytes(b ^ i for b in file_key)
        h = _rc4(k, h)
    return (h + bytes(16))[:32]


def _obj_key(file_key: bytes, num: int, gen: int) -> bytes:
    md = hashlib.md5()
    md.update(file_key)
    md.update(struct.pack("<I", num)[:3])
    md.update(struct.pack("<I", gen)[:2])
    return md.digest()[: min(len(file_key) + 5, 16)]


def build_encrypted_pdf(user_pw: bytes = b"", owner_pw: bytes = b"owner") -> bytes:
    """A one-page RC4 V2/R3 128-bit encrypted PDF with an encrypted /Info /Title."""
    key_len = 16
    p = -44
    id0 = b"0123456789abcdef"
    o = _compute_o(owner_pw, user_pw, key_len)
    fk = _file_key(user_pw, o, p, id0, key_len)
    u = _compute_u(fk, id0)

    title_plain = b"Secret Title"
    title_enc = _rc4(_obj_key(fk, 5, 0), title_plain)

    def hexstr(b: bytes) -> bytes:
        return b"<" + b.hex().encode() + b">"

    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 100 100] >>"),
        (3, b"<< /Type /Page /Parent 2 0 R >>"),
        (5, b"<< /Title " + hexstr(title_enc) + b" >>"),
        (
            6,
            b"<< /Filter /Standard /V 2 /R 3 /Length 128 /P "
            + str(p).encode()
            + b" /O "
            + hexstr(o)
            + b" /U "
            + hexstr(u)
            + b" >>",
        ),
    ]

    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    max_num = 0
    for num, body in objects:
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
        max_num = max(max_num, num)

    size = max_num + 1
    startxref = len(out)
    out += b"xref\n" + f"0 {size}\n".encode() + b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in offsets:
            out += f"{offsets[num]:010} 00000 n \n".encode()
        else:
            out += b"0000000000 65535 f \n"
    out += b"trailer\n"
    out += (
        b"<< /Size "
        + str(size).encode()
        + b" /Root 1 0 R /Info 5 0 R /Encrypt 6 0 R /ID ["
        + hexstr(id0)
        + b" "
        + hexstr(id0)
        + b"] >>\n"
    )
    out += b"startxref\n" + f"{startxref}\n".encode() + b"%%EOF\n"
    return bytes(out)


@pytest.fixture()
def encrypted_path(tmp_path):
    p = tmp_path / "encrypted.pdf"
    p.write_bytes(build_encrypted_pdf())
    return str(p)


def test_pyfitz_002_encrypted_flow(encrypted_path):
    # PYFITZ-002: needs_pass → authenticate → pages + decrypted metadata.
    import fitz

    doc = fitz.open(encrypted_path)
    assert doc.is_encrypted is True
    assert doc.needs_pass is True
    assert doc.permissions == -44
    md = doc.metadata
    assert md["encryption"].startswith("Standard")

    assert doc.authenticate("") is True
    assert doc.needs_pass is False
    assert doc.page_count == 1
    page = doc[0]
    assert tuple(page.rect) == (0.0, 0.0, 100.0, 100.0)
    # The /Info /Title now decrypts.
    assert doc.metadata["title"] == "Secret Title"


def test_encrypted_wrong_password(encrypted_path):
    doc = oxipdf.open(encrypted_path)
    assert doc.needs_pass is True
    assert doc.authenticate("definitely-wrong") is False
    assert doc.needs_pass is True
