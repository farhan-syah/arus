#!/usr/bin/env python3
"""Check public repository paths without reading private file contents."""

from pathlib import PurePosixPath
import subprocess
import sys


def main() -> int:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        check=True,
        stdout=subprocess.PIPE,
    )
    local_roots = {
        ".aws",
        ".local",
        "artifacts",
        "data",
        "reference",
        "resouce",
        "resource",
        "resources",
        "scratch",
        "target",
    }
    local_prefixes = {".agents/prep", ".codex/prep"}
    rejected = []
    for raw_path in result.stdout.split(b"\0"):
        if not raw_path:
            continue
        name = raw_path.decode("utf-8", errors="surrogateescape")
        path = PurePosixPath(name)
        private = path.parts[0] in local_roots
        private |= name == ".cargo/config.toml"
        private |= any(
            name == prefix or name.startswith(prefix + "/")
            for prefix in local_prefixes
        )
        private |= path.name == ".env" or (
            path.name.startswith(".env.") and path.name != ".env.example"
        )
        if private:
            rejected.append(name)
    if rejected:
        for name in sorted(set(rejected)):
            print(
                f"Public repository path check rejected {name!r}. "
                "Remove it from Git and ignore it.",
                file=sys.stderr,
            )
        return 1
    print("Public repository path check passes.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
