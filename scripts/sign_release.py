#!/usr/bin/env python3
"""
أداة توليد وتوقيع بيان التحديث الذاتي الرسمي (version.json + SHA256SUMS + SHA2-256SUMS) لـ StormGet v2.3.6
باستخدام خوارزمية Ed25519 وبصمة SHA-256 للمثبت (متوافقة 100% مع src/core/selfupdate.rs).

ملاحظة أمنية:
لا يُمرر المفتاح السري أبداً كمعامل في سطر الأوامر؛ بل يُقرأ حصرياً من متغير البيئة:
  export STORMGET_SIGNING_SEED_HEX="<64-hex-chars>"
أو من ملف مفتاح محمي عبر:
  python scripts/sign_release.py --installer dist/StormGet-v2.3.6-Setup-x64.exe --url https://github.com/StormGet/StormGet/releases/download/v2.3.6/StormGet-Setup.exe --key-file ~/.stormget_signing.key
"""
import argparse
import hashlib
import os
import subprocess
import sys


def compute_sha256(file_path: str) -> str:
    h = hashlib.sha256()
    with open(file_path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def main():
    parser = argparse.ArgumentParser(description="Sign StormGet release installer safely without exposing private seed on CLI")
    parser.add_argument("--installer", required=True, help="Path to compiled StormGet Setup .exe")
    parser.add_argument("--version", default="2.3.6", help="Release version (e.g. 2.3.0)")
    parser.add_argument("--url", required=True, help="HTTPS download URL for the installer")
    parser.add_argument("--notes", default="StormGet v2.3.6 Final Gold Release", help="Release notes")
    parser.add_argument("--output", default="dist/version.json", help="Output path for signed version.json")
    parser.add_argument("--key-file", help="Path to protected file containing 64-char hex Ed25519 seed")
    parser.add_argument("--generate-key-out", help="Generate a new keypair and save private seed directly to this file (never printed)")
    args = parser.parse_args()

    if not os.path.exists(args.installer):
        print(f"Error: installer file not found: {args.installer}", file=sys.stderr)
        sys.exit(1)

    sha256_hex = compute_sha256(args.installer)
    print(f"[+] Computed SHA-256: {sha256_hex}")
    cmd = [
        "cargo", "run", "--release", "--",
        "sign-release",
        "--version", args.version,
        "--url", args.url,
        "--installer", args.installer,
        "--notes", args.notes,
        "--output", args.output,
    ]
    if args.key_file:
        cmd.extend(["--key-file", args.key_file])
    if args.generate_key_out:
        cmd.extend(["--generate-key-out", args.generate_key_out])

    subprocess.run(cmd, check=True)


if __name__ == "__main__":
    main()
