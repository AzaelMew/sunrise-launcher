"""Helpers the launcher's own test tools share (the game repository keeps the full harness)."""
import atexit
import os
import shutil
import tempfile

_SCRATCH_DIRS = []


def _scratch_dir(prefix):
    path = tempfile.mkdtemp(prefix=prefix)
    _SCRATCH_DIRS.append(path)
    return path


@atexit.register
def _remove_scratch_dirs():
    for path in _SCRATCH_DIRS:
        shutil.rmtree(path, ignore_errors=True)


def game_env(base=None, **extra):
    """The environment for a run of the launcher from a tool: no audio, no self-update, loopback-only fixtures,
    and fresh scratch folders for the game's home, the launcher's home and the XDG folders, so a run never writes
    into a real player's folders. A folder the caller already has in `base` (default os.environ) or passes in
    `extra` is kept. The scratch folders are removed when this process exits."""
    env = dict(os.environ if base is None else base)
    env.update({key: str(value) for key, value in extra.items()})
    env["SUNRISE_NO_AUDIO"] = "1"
    env["SUNRISE_LAUNCHER_NO_SELF_UPDATE"] = "1"
    env.setdefault("SUNRISE_TEST_LOOPBACK", "1")
    for key, prefix in (("SUNRISE_HOME", "sunrise-home-"), ("SUNRISE_LAUNCHER_HOME", "sunrise-launcher-home-"),
                        ("XDG_DATA_HOME", "sunrise-xdg-data-"), ("XDG_CONFIG_HOME", "sunrise-xdg-config-")):
        if not env.get(key):
            env[key] = _scratch_dir(prefix)
    return env
