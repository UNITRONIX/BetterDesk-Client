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

if failures:
    print("\n".join(failures), file=sys.stderr)
    sys.exit(1)

print("Desktop packaging contract: OK")
