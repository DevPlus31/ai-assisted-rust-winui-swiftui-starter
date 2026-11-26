#!/usr/bin/env python3
"""Optional Mac App Store pre-flight checks on a built .app.

Usage: python3 -I scripts/store-check-macos.py "path/to/App Starter.app"
       (or: make store-check-macos)

Checks what a machine can check before App Review: code signature,
sandbox and entitlements, Info.plist keys, the privacy manifest against
the "required reason" APIs the binary actually imports, and the binary's
imports for networking and runtime code loading. It cannot predict human
review. Exit code 1 when any check fails; warnings don't fail.
"""

import plistlib
import re
import subprocess
import sys
from pathlib import Path

# Entitlements the sample app may have. Anything else needs a reason.
ALLOWED_ENTITLEMENTS = {
    "com.apple.security.app-sandbox",
    "com.apple.security.files.user-selected.read-write",
    "com.apple.security.files.user-selected.read-only",
    "com.apple.security.files.bookmarks.app-scope",
}
NETWORK_ENTITLEMENTS = {
    "com.apple.security.network.client",
    "com.apple.security.network.server",
}
# Imports that mean networking or loading code at runtime (guideline 2.5.2).
# `dlsym` alone is allowed: it only finds symbols in already-loaded system
# libraries (Rust crates use it to detect newer libc functions).
FORBIDDEN_IMPORTS = {
    "_socket": "networking",
    "_connect": "networking",
    "_bind": "networking",
    "_listen": "networking",
    "_accept": "networking",
    "_getaddrinfo": "networking",
    "_sendto": "networking",
    "_recvfrom": "networking",
    "_dlopen": "loading code at runtime",
    "_NSCreateObjectFileImageFromMemory": "loading code at runtime",
}
FORBIDDEN_SUBSTRINGS = {
    "URLSession": "networking",
    "NSURLConnection": "networking",
    "nw_connection": "networking",
    "CFSocket": "networking",
}
# Apple's required-reason API categories, detected from imported symbols.
# https://developer.apple.com/documentation/bundleresources/describing-use-of-required-reason-api
REQUIRED_REASON_SYMBOLS = {
    "NSPrivacyAccessedAPICategoryFileTimestamp": re.compile(
        r"^_(f?stat(at)?|lstat|f?getattrlist(at|bulk)?)(\$INODE64)?$"
        r"|NSURLContentModificationDateKey|NSURLCreationDateKey|NSFileModificationDate|NSFileCreationDate"
    ),
    "NSPrivacyAccessedAPICategorySystemBootTime": re.compile(r"^_mach_absolute_time$"),
    "NSPrivacyAccessedAPICategoryDiskSpace": re.compile(
        r"^_f?stat(v)?fs(\$INODE64)?$|NSURLVolumeAvailableCapacity|NSFileSystemFreeSize|NSFileSystemSize"
    ),
    "NSPrivacyAccessedAPICategoryUserDefaults": re.compile(r"NSUserDefaults"),
}


class Report:
    def __init__(self) -> None:
        self.failures = 0

    def ok(self, message: str) -> None:
        print(f"  ✓ {message}")

    def warn(self, message: str) -> None:
        print(f"  ! {message}")

    def fail(self, message: str) -> None:
        print(f"  ✗ {message}")
        self.failures += 1


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(args, capture_output=True, text=False, check=False)


def main_binary(app: Path, info: dict) -> Path:
    return app / "Contents" / "MacOS" / info.get("CFBundleExecutable", "")


def check_signature(app: Path, report: Report) -> None:
    print("Code signature")
    result = run("codesign", "--verify", "--strict", "--deep", str(app))
    if result.returncode == 0:
        report.ok("signature is valid (strict, deep)")
    else:
        report.fail(f"codesign --verify failed: {result.stderr.decode().strip()}")
    details = run("codesign", "-dv", str(app)).stderr.decode()
    if "flags=0x10000(runtime)" in details or "(runtime)" in details:
        report.ok("hardened runtime is on")
    else:
        report.warn("hardened runtime is off (needed for notarized builds outside the store)")


def check_entitlements(app: Path, report: Report) -> None:
    print("Sandbox and entitlements")
    result = run("codesign", "-d", "--entitlements", ":-", str(app))
    try:
        entitlements = plistlib.loads(result.stdout) if result.stdout.strip() else {}
    except plistlib.InvalidFileException:
        report.fail("could not read the entitlements")
        return
    if entitlements.get("com.apple.security.app-sandbox") is True:
        report.ok("App Sandbox is on (required for the Mac App Store)")
    else:
        report.fail("App Sandbox is off: com.apple.security.app-sandbox must be true")
    for key in sorted(entitlements):
        if key in NETWORK_ENTITLEMENTS:
            report.fail(f"network entitlement {key}: the engine promises to run locally")
        elif key == "com.apple.security.get-task-allow":
            report.warn("get-task-allow is set (debug signing); App Store export removes it")
        elif key not in ALLOWED_ENTITLEMENTS:
            report.fail(f"unexpected entitlement {key}: add it to the allow list with a reason")
    if not any(key in NETWORK_ENTITLEMENTS for key in entitlements):
        report.ok("no network entitlements")


def check_info_plist(info: dict, report: Report) -> None:
    print("Info.plist")
    bundle_id = info.get("CFBundleIdentifier", "")
    if not bundle_id:
        report.fail("CFBundleIdentifier is missing")
    elif bundle_id.startswith("com.example."):
        report.warn(f"bundle ID {bundle_id} is a placeholder; set your own before submitting")
    else:
        report.ok(f"bundle ID {bundle_id}")
    version = info.get("CFBundleShortVersionString", "")
    if re.fullmatch(r"\d+(\.\d+){0,2}", version):
        report.ok(f"version {version}")
    else:
        report.fail(f"CFBundleShortVersionString '{version}' must be like 1.2.3")
    if str(info.get("CFBundleVersion", "")).strip():
        report.ok(f"build number {info['CFBundleVersion']}")
    else:
        report.fail("CFBundleVersion (build number) is missing")
    if info.get("LSApplicationCategoryType"):
        report.ok(f"category {info['LSApplicationCategoryType']}")
    else:
        report.fail("LSApplicationCategoryType is missing (required by App Store Connect)")
    if info.get("LSMinimumSystemVersion"):
        report.ok(f"minimum macOS {info['LSMinimumSystemVersion']}")
    else:
        report.fail("LSMinimumSystemVersion is missing")


def imported_symbols(binary: Path) -> set[str]:
    archs = run("lipo", "-archs", str(binary)).stdout.decode().split() or [""]
    symbols: set[str] = set()
    for arch in archs:
        args = ["nm", "-u"] + (["-arch", arch] if arch else []) + [str(binary)]
        symbols.update(run(*args).stdout.decode().split())
    return symbols


def check_imports(binary: Path, symbols: set[str], report: Report) -> None:
    print("Binary imports")
    archs = run("lipo", "-archs", str(binary)).stdout.decode().strip()
    report.ok(f"architectures: {archs}")
    problems = [
        f"{symbol} ({reason})" for symbol, reason in FORBIDDEN_IMPORTS.items() if symbol in symbols
    ] + [
        f"{symbol} ({reason})"
        for symbol in sorted(symbols)
        for part, reason in FORBIDDEN_SUBSTRINGS.items()
        if part in symbol
    ]
    if problems:
        for problem in problems:
            report.fail(f"imports {problem}")
    else:
        report.ok("no networking or runtime code loading imports")


def check_privacy_manifest(app: Path, symbols: set[str], report: Report) -> None:
    print("Privacy manifest")
    path = app / "Contents" / "Resources" / "PrivacyInfo.xcprivacy"
    if not path.is_file():
        report.fail("Contents/Resources/PrivacyInfo.xcprivacy is missing")
        return
    manifest = plistlib.loads(path.read_bytes())
    if manifest.get("NSPrivacyTracking") is False:
        report.ok("declares no tracking")
    else:
        report.fail("NSPrivacyTracking must be false: the app doesn't track users")
    declared = {
        entry.get("NSPrivacyAccessedAPIType"): entry.get("NSPrivacyAccessedAPITypeReasons", [])
        for entry in manifest.get("NSPrivacyAccessedAPITypes", [])
    }
    for category, pattern in REQUIRED_REASON_SYMBOLS.items():
        used = sorted(symbol for symbol in symbols if pattern.search(symbol))
        short = category.removeprefix("NSPrivacyAccessedAPICategory")
        if used and not declared.get(category):
            report.fail(f"uses {short} APIs ({', '.join(used[:3])}) but declares no reason")
        elif used:
            report.ok(f"{short}: used and declared ({', '.join(declared[category])})")
        elif category in declared:
            report.warn(f"{short} is declared but no matching import was found")


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    app = Path(sys.argv[1])
    info_path = app / "Contents" / "Info.plist"
    if not info_path.is_file():
        print(f"error: {app} is not an app bundle")
        return 2
    info = plistlib.loads(info_path.read_bytes())
    binary = main_binary(app, info)
    symbols = imported_symbols(binary)

    print(f"Mac App Store pre-flight: {app.name}\n")
    report = Report()
    check_signature(app, report)
    check_entitlements(app, report)
    check_info_plist(info, report)
    check_imports(binary, symbols, report)
    check_privacy_manifest(app, symbols, report)

    print()
    if report.failures:
        print(f"{report.failures} check(s) failed.")
        return 1
    print("All checks passed. App Review can still reject for reasons no tool can check.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
