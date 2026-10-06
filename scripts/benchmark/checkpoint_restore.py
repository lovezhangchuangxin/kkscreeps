#!/usr/bin/env python3
"""用当前 release runtime 逐值核验旧检查点，再续跑真实玩家；保留 sim 的 CLI 身份门。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packages", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("checkpoints", type=Path, nargs="+")
    args = parser.parse_args()
    build = subprocess.run(["cargo", "build", "--release", "-p", "kk-cli", "--message-format=json"],
                           cwd=ROOT, check=True, text=True, stdout=subprocess.PIPE)
    libraries = {}
    for line in build.stdout.splitlines():
        row = json.loads(line)
        if row.get("reason") == "compiler-artifact" and row["target"]["name"] in ("kk_runtime", "kk_mod_host", "serde_json"):
            libraries[row["target"]["name"]] = next(path for path in row["filenames"] if path.endswith(".rlib"))
    if len(libraries) != 3:
        raise ValueError("当前 release 依赖身份不完整")
    with tempfile.TemporaryDirectory(prefix="kks-checkpoint-") as directory:
        executable = Path(directory) / "restore"
        command = ["rustc", "--edition=2024", str(Path(__file__).with_suffix(".rs")), "-L",
                   "dependency=" + str(Path(libraries["kk_runtime"]).parent), "-o", str(executable)]
        for name, path in libraries.items():
            command += ["--extern", f"{name}={path}"]
        subprocess.run(command, cwd=ROOT, check=True)
        run = subprocess.run([str(executable), str(args.packages.resolve()),
                              *[str(path.resolve()) for path in args.checkpoints]], cwd=ROOT,
                             check=True, text=True, stdout=subprocess.PIPE)
    result = dict(restores=json.loads(run.stdout),
                  library_sha256={name: hashlib.sha256(Path(path).read_bytes()).hexdigest() for name, path in libraries.items()},
                  fixture_sha256={str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in args.checkpoints})
    with args.report.open("x") as output:
        json.dump(result, output, ensure_ascii=False, indent=2)
        output.write("\n")
    print(run.stdout)


if __name__ == "__main__":
    main()
