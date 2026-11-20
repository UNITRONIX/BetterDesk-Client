#!/usr/bin/env python3
"""Check the source-level launcher and installer contract for desktop builds."""

from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def require(relative: str, text: str, needle: str) -> None:
    if needle not in text:
        failures.append(f"{relative}: missing {needle!r}")


failures: list[str] = []

linux_cmake = read("flutter/linux/CMakeLists.txt")
require("flutter/linux/CMakeLists.txt", linux_cmake, 'set(BINARY_NAME "betterdesk")')

service = read("res/rustdesk.service")
require("res/rustdesk.service", service, "ExecStart=/usr/bin/betterdesk --service")
require("res/rustdesk.service", service, 'ExecStop=/usr/bin/pkill -f "betterdesk --"')

postinst = read("res/DEBIAN/postinst")
require("res/DEBIAN/postinst", postinst, "ln -f -s /usr/share/rustdesk/betterdesk /usr/bin/betterdesk")
require("res/DEBIAN/postinst", postinst, "ln -f -s /usr/share/rustdesk/betterdesk /usr/bin/rustdesk")

desktop = read("res/betterdesk.desktop")
require("res/betterdesk.desktop", desktop, "Exec=betterdesk %u")
require("res/betterdesk.desktop", desktop, "StartupWMClass=betterdesk")

for relative in ("res/betterdesk-link.desktop", "res/rustdesk-link.desktop"):
    require(relative, read(relative), "TryExec=betterdesk")
    require(relative, read(relative), "Exec=betterdesk %u")

build = read("build.py")
for needle in (
    "cp ../res/betterdesk.desktop tmpdeb/usr/share/applications/betterdesk.desktop",
    "cp ../res/betterdesk-link.desktop tmpdeb/usr/share/applications/betterdesk-link.desktop",
    "dpkg-deb -b tmpdeb betterdesk.deb;",
):
    require("build.py", build, needle)

for relative in ("res/rpm-flutter.spec", "res/rpm-flutter-suse.spec"):
    spec = read(relative)
    require(relative, spec, "Name:       betterdesk")
    require(relative, spec, "ln -sf /usr/share/rustdesk/betterdesk /usr/bin/betterdesk")

for relative in ("appimage/AppImageBuilder-x86_64.yml", "appimage/AppImageBuilder-aarch64.yml"):
    appimage = read(relative)
    require(relative, appimage, 'bsdtar -xvf "$DEB"')
    require(relative, appimage, "exec: usr/share/rustdesk/betterdesk")
    require(relative, appimage, "name: BetterDesk")

flatpak = read("flatpak/rustdesk.json")
require("flatpak/rustdesk.json", flatpak, '"command": "betterdesk"')
require("flatpak/rustdesk.json", flatpak, "/app/share/rustdesk/betterdesk")
require("flatpak/rustdesk.json", flatpak, "/app/share/metainfo/com.unitronix.betterdesk.metainfo.xml")

pkgbuild = read("res/PKGBUILD")
require("res/PKGBUILD", pkgbuild, 'install -Dm 755 "${HBB}/target/release/rustdesk" "${pkgdir}/usr/share/rustdesk/betterdesk"')

preprocess = read("res/msi/preprocess.py")
require("res/msi/preprocess.py", preprocess, 'default="BetterDesk Client"')
require("res/msi/preprocess.py", preprocess, '"betterdesk.exe"')
require("res/msi/preprocess.py", preprocess, 'UriScheme="betterdesk"')
# Install identity: the service name must never come from the display name (spaces),
# and the UpgradeCode must be stable so a rebrand upgrades instead of duplicating.
require("res/msi/preprocess.py", preprocess, '--install-name')
require("res/msi/preprocess.py", preprocess, '<?define InstallName="{args.install_name}" ?>')
require("res/msi/preprocess.py", preprocess, 'ProductLower="{args.install_name.lower()}"')
require("res/msi/preprocess.py", preprocess, 'PRODUCT_UPGRADE_CODE')
require("res/msi/preprocess.py", preprocess, 'unitronix.betterdesk.desktop')
require("res/msi/preprocess.py", preprocess, 'LEGACY_UPGRADE_CODES')
if "app_name + \".exe\"" in preprocess:
    failures.append(
        "res/msi/preprocess.py: UpgradeCode must not be derived from --app-name"
    )
if "datetime.datetime.now().timestamp()" in preprocess:
    failures.append(
        "res/msi/preprocess.py: default MSI revision must not depend on the wall clock"
    )
rust_common = read("src/common.rs")
require("src/common.rs", rust_common, "install_app_name_from_display")
windows_wxs = read("res/msi/Package/Components/RustDesk.wxs")
require(
    "res/msi/Package/Components/RustDesk.wxs",
    windows_wxs,
    'Property="CreateStartService" Value="$(var.InstallName);',
)
require(
    "res/msi/Package/Components/RustDesk.wxs",
    windows_wxs,
    'Property="TryStopDeleteService" Value="$(var.InstallName);$(var.LegacyServiceName)"',
)
if 'Property="CreateStartService" Value="$(var.Product);' in windows_wxs:
    failures.append(
        "res/msi/Package/Components/RustDesk.wxs: service name must use $(var.InstallName), not $(var.Product)"
    )
build_py = read("build.py")
if "return ''\n\n\ndef parse_rc_features" in build_py:
    failures.append("build.py: get_version() must not return an empty version")
regs = read("res/msi/Package/Components/Regs.wxs")
require("res/msi/Package/Components/Regs.wxs", regs, 'Key="$(var.UriScheme)"')
require("res/msi/Package/Components/Regs.wxs", regs, 'Key="$(var.LegacyUriScheme)"')
require("res/msi/Package/Components/RustDesk.wxs", read("res/msi/Package/Components/RustDesk.wxs"), 'Name="$(var.Executable)"')

mac_xcconfig = read("flutter/macos/Runner/Configs/AppInfo.xcconfig")
require("flutter/macos/Runner/Configs/AppInfo.xcconfig", mac_xcconfig, "PRODUCT_NAME = BetterDesk Client")
require(
    "flutter/macos/Runner/Configs/AppInfo.xcconfig",
    mac_xcconfig,
    "PRODUCT_BUNDLE_IDENTIFIER = com.unitronix.betterdesk",
)
mac_plist = read("flutter/macos/Runner/Info.plist")
require("flutter/macos/Runner/Info.plist", mac_plist, "<string>betterdesk</string>")
require("flutter/macos/Runner/Info.plist", mac_plist, "<string>rustdesk</string>")
macos_runtime = read("src/platform/macos.rs")
require("src/platform/macos.rs", macos_runtime, 'com.unitronix.{}_service.plist')
require("src/platform/macos.rs", macos_runtime, 'com.unitronix.{}_server.plist')

release = read(".github/workflows/betterdesk-desktop-release.yml")
require(".github/workflows/betterdesk-desktop-release.yml", release, 'betterdesk-unsigned-macos-${arch}')

# ---------------------------------------------------------------------------
# Issue #516: the generic Linux bundle is built with `dpkg-deb -x`, which
# extracts only the data archive. The deb maintainer script (postinst) is what
# creates /usr/bin/betterdesk, so it never runs and the tarball shipped an
# empty usr/bin while its .desktop entries still ran `Exec=betterdesk %u`.
# Nothing started after unpacking. These assertions keep the packaging step
# honest: the launcher must be created explicitly, and it must be a relative
# symlink because postinst hardcodes /usr/share/rustdesk/... paths that are
# invalid for an archive unpacked under any other prefix.
# ---------------------------------------------------------------------------
generic_release = read(".github/workflows/betterdesk-support-generic-release.yml")
require(
    ".github/workflows/betterdesk-support-generic-release.yml",
    generic_release,
    "ln -sfn ../share/rustdesk/betterdesk",
)
require(
    ".github/workflows/betterdesk-support-generic-release.yml",
    generic_release,
    "install -d",
)
if "dpkg-deb -x" not in generic_release:
    failures.append(
        "betterdesk-support-generic-release.yml: expected package_linux to extract the deb with dpkg-deb -x"
    )
# The bundle must not gain a step that runs absolute-path maintainer scripts.
for banned in ("dpkg -i", "dpkg --install", "dpkg-deb -e"):
    if banned in generic_release:
        failures.append(
            f"betterdesk-support-generic-release.yml: must not run {banned!r}; "
            "it uses absolute /usr/share/rustdesk paths that break unpack-anywhere archives"
        )
# A shipped installer that honours PREFIX, plus a short README.
require(".github/workflows/betterdesk-support-generic-release.yml", generic_release, "install.sh")
require(".github/workflows/betterdesk-support-generic-release.yml", generic_release, 'PREFIX="${PREFIX:-/usr/local}"')
require(".github/workflows/betterdesk-support-generic-release.yml", generic_release, "README.txt")
# The launcher target must be verified to exist, not assumed.
require(".github/workflows/betterdesk-support-generic-release.yml", generic_release, 'test -x "${rootdir}/usr/share/rustdesk/betterdesk"')

if failures:
    print("\n".join(failures), file=sys.stderr)
    sys.exit(1)

print("Desktop packaging contract: OK")
