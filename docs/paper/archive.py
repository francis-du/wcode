"""Archive the paper's fixed diagnostic report; no experiments or overwrites."""
from pathlib import Path
import hashlib
import json
import os
import stat

ROOT = Path(__file__).resolve().parents[2]
PAPER = ROOT / "docs" / "paper"
SOURCE = ROOT / "target" / "engineering-fitness-1789665000441866000-37653.json"
DESTINATION = PAPER / "fitness.raw.json"
EXPECTED = "b117266f8c33cebcef26cb2fc9d56a5bf50b48f604d67ffab1944a16b4807ffc"
SIZE = 597957


def read_regular(path: Path) -> bytes:
    """Refuse link components and oversized or non-regular inputs."""
    relative = path.relative_to(ROOT)
    current = ROOT
    for part in relative.parts:
        current = current / part
        if current.is_symlink():
            raise ValueError(f"symlink refused: {relative}")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    with os.fdopen(os.open(path, flags), "rb") as handle:
        info = os.fstat(handle.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size != SIZE:
            raise ValueError("unexpected report type or size")
        data = handle.read(SIZE + 1)
    if len(data) != SIZE or hashlib.sha256(data).hexdigest() != EXPECTED:
        raise ValueError("canonical report fingerprint mismatch")
    return data


def main() -> None:
    if DESTINATION.exists() or DESTINATION.is_symlink():
        read_regular(DESTINATION)
        outcome = "existing_identical_archive"
    else:
        data = read_regular(SOURCE)
        for path in [ROOT / "docs", PAPER]:
            if path.is_symlink() or not path.is_dir():
                raise ValueError("invalid archive directory")
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
        with os.fdopen(os.open(DESTINATION, flags, 0o644), "wb") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        read_regular(DESTINATION)
        outcome = "archived"
    print(json.dumps({"status": outcome, "path": str(DESTINATION.relative_to(ROOT)),
                      "bytes": SIZE, "sha256": EXPECTED, "new_experiment": False}))


if __name__ == "__main__":
    main()
