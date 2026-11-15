#!/usr/bin/env python3
"""Stage repository-specific launcher packaging without modifying upstream templates.

The returned directory contains the existing packaging scripts and their private inputs.
Run linux/appimage.sh or macos/package.sh from that directory as usual.
"""
import argparse
import shutil
from pathlib import Path

UPSTREAM = 'horizon-rust/horizon'
FORK = 'sunrise-org/sunrise'

def stage(source: Path, destination: Path, repository: str):
    if repository not in (UPSTREAM, FORK):
        raise ValueError(f'Unsupported launcher repository: {repository}')
    shutil.copytree(source, destination)
    if repository == UPSTREAM:
        return
    replacements = {
        'horizon-launcher': 'horizon-fork-launcher',
        'horizon-fork-launcher.desktop': 'net.playsunrise.launcher.desktop',
        'dev.horizon.launcher': 'net.playsunrise.launcher',
        'Horizon.app': 'Sunrise Launcher.app',
        '<string>Horizon</string>': '<string>Sunrise Launcher</string>',
        'Name=Horizon\n': 'Name=Sunrise Launcher\n',
        'StartupWMClass=horizon-fork-launcher': 'StartupWMClass=net.playsunrise.launcher',
    }
    for name in ('linux/appimage.sh', 'linux/horizon-launcher.desktop', 'macos/package.sh', 'macos/Info.plist'):
        path = destination / name
        text = path.read_text()
        for old, new in replacements.items():
            text = text.replace(old, new)
        if name == 'macos/package.sh':
            # Spaces in the fork's bundle name must remain one shell argument.
            text = text.replace('app=$out/Sunrise Launcher.app', 'app="$out/Sunrise Launcher.app"')
            text = text.replace('--keepParent Sunrise Launcher.app', '--keepParent "Sunrise Launcher.app"')
            text = text.replace('identity=${MACOS_SIGN_IDENTITY:--}', 'identity=-')
            text = text.replace('"title": "Horizon"', '"title": "Sunrise Launcher"')
            text = text.replace('"$out/dmg.json" Horizon "$dmg"', '"$out/dmg.json" "Sunrise Launcher" "$dmg"')
        path.write_text(text)
    (destination / 'linux/horizon-launcher.desktop').rename(destination / 'linux/net.playsunrise.launcher.desktop')
    shutil.copyfile(source / 'icons/fork/launcher.ico', destination / 'icons/horizon.ico')
    shutil.copyfile(source / 'icons/fork/launcher.icns', destination / 'icons/horizon.icns')
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        shutil.copyfile(source / f'icons/fork/{size}.png', destination / f'icons/hicolor/{size}x{size}/apps/horizon.png')
    shutil.copyfile(source / 'icons/fork/launcher.svg', destination / 'icons/hicolor/scalable/apps/horizon.svg')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--repository', default=FORK, choices=(UPSTREAM, FORK))
    args = parser.parse_args()
    stage(Path(__file__).resolve().parents[1] / 'packaging', args.destination, args.repository)
