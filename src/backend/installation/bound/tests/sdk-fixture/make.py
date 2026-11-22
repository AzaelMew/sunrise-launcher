#!/usr/bin/env python3
"""Regenerate the tiny SDK install archives. All files contain synthetic test text."""
import io
import tarfile
from pathlib import Path

HERE = Path(__file__).parent


def archive(name, paths):
    with tarfile.open(HERE / f"{name}.tar.xz", "w:xz") as tar:
        for path in paths:
            data = (path + "\n").encode()
            entry = tarfile.TarInfo(path)
            entry.size = len(data)
            entry.mode = 0o755 if Path(path).name.startswith("sunrise-") else 0o644
            tar.addfile(entry, io.BytesIO(data))


programs = ["sunrise-prepare", "sunrise-server", "sunrise-ctl", "sunrise-perf", "sunrise-port-pack"]
companions = [
    "exporters/manifest.json", "exporters/client/fixture.jar", "exporters/server/fixture.jar",
    "research-lock.json", "LICENSE", "LICENSES/MIT-Horizon.txt", "PLAY.txt",
    "tools/fixture.py", "crates/sunrise-prepare/fixture.rs", "mods/sunrise/src/test/java/Fixture.java",
]
archive("unix-programs", programs + companions)
archive("windows-programs", [name + ".exe" for name in programs] + companions + [
    "dxcompiler.dll", "DXC-LICENSE-MS.txt", "DXC-LICENSE-LLVM.txt", "DXC-NOTICE.txt",
])
for system, extension in [("linux", "so"), ("macos", "dylib")]:
    archive(f"{system}-runtime", ["bin/sunrise-client", f"bin/libstd-fixture.{extension}",
                                  f"deps/libsunrise_fixture.{extension}", "deps/liblink-only.a",
                                  "sunrise-prebuilt.json"])
archive("windows-runtime", [
    "bin/sunrise-client.exe", "bin/std-fixture.dll", "bin/libgcc_s_seh-1.dll",
    "bin/libwinpthread-1.dll", "deps/sunrise_fixture.dll", "deps/libsunrise_fixture.dll.a",
    "deps/liblink-only.a", "sunrise-prebuilt.json",
])
