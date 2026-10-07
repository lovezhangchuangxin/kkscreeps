"""Recompute short-window summaries; no external packages or GC assumptions."""
import base64
import csv
import gzip
import hashlib
import json
import math
import sys
from pathlib import Path


def stats(rows, key):
    values = sorted(row[key] for row in rows if isinstance(row, dict)
                    and isinstance(row.get(key), (int, float)) and not isinstance(row[key], bool))
    if not values:
        return None
    return {"n": len(values), "p50": values[math.ceil(len(values) * .5) - 1],
            "p95": values[math.ceil(len(values) * .95) - 1], "max": values[-1]}


def summarize(rows):
    keys = sorted({key for row in rows if isinstance(row, dict) for key in row if key != "i"})
    return {key: value for key in keys if (value := stats(rows, key)) is not None}


def walk(value, path, output):
    if not isinstance(value, dict):
        return
    records = value.get("records")
    if isinstance(records, list) and records:
        if isinstance(records[0], dict):
            output[path] = summarize(records)
            updates = [row for row in records if row.get("visualMs", 0) > 0]
            if updates:
                output[path]["nonzeroVisualUpdateMs"] = stats(updates, "visualMs")
            if "rebuild" in records[0]:
                output[path]["arms"] = {str(mode): summarize([row for row in records if row["rebuild"] is mode])
                                        for mode in (True, False)}
            for nested in ("apply", "feed"):
                rows = [part for row in records for part in row.get(nested, [])]
                if rows:
                    output[path][nested] = summarize(rows)
                    if nested == "feed":
                        output[path]["feedByOperation"] = {operation: summarize([row for row in rows if row["operation"] == operation])
                                                           for operation in sorted({row["operation"] for row in rows})}
        elif isinstance(records[0], (int, float)):
            output[path] = stats([{"value": value} for value in records], "value")
    for key, child in value.items():
        if isinstance(child, dict):
            walk(child, f"{path}.{key}", output)
        elif key == "trials" and isinstance(child, list):
            for i, trial in enumerate(child):
                walk(trial, f"{path}.{key}[{i}]", output)


def main():
    directory = Path(sys.argv[1]).resolve()
    output, curves = {}, []
    for path in sorted(directory.glob("*.json")):
        if path.name in ("summary.json", "pixel-differences.json", "environment.json", "regeneration.json"):
            continue
        data = json.loads(path.read_text())
        if not isinstance(data, dict) or "data" not in data:
            continue
        per_file = {}
        walk(data["data"], "data", per_file)
        output[path.name] = per_file
        for window in data["data"].get("windows", []):
            counts = window["sample"]["counts"]
            for index, heap in enumerate(window["sample"]["heapBytes"]):
                curves.append({"file": path.name, "tick": window["tick"], "sample": index,
                               "heapBytes": heap, **{key: counts[key] for key in
                                ("graphicsContexts", "graphicsContextSlots", "sources", "estimatedSourceBytes", "bindGroups", "nullSlots")}})
    (directory / "summary.json").write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n")
    if curves:
        with (directory / "memory-windows.csv").open("w", newline="") as file:
            writer = csv.DictWriter(file, fieldnames=list(curves[0]), lineterminator="\n")
            writer.writeheader()
            writer.writerows(curves)
    comparisons = []
    for backend in ("webgpu", "webgl"):
        baseline = json.loads((directory / f"{backend}-before-pixels-raw.json").read_text())["data"]["records"]
        final = json.loads((directory / f"{backend}-after-v3-pixels-raw.json").read_text())["data"]["records"]
        assert len(baseline) == len(final) == 8
        for a, b in zip(baseline, final):
            raw_a, raw_b = (gzip.decompress(base64.b64decode(row["rgbaGzipBase64"])) for row in (a, b))
            assert (a["width"], a["height"], a["count"]) == (b["width"], b["height"], b["count"])
            assert len(raw_a) == len(raw_b) == a["width"] * a["height"] * 4
            assert hashlib.sha256(raw_a).hexdigest() == a["sha256"]
            assert hashlib.sha256(raw_b).hexdigest() == b["sha256"]
            diffs = [abs(x - y) for x, y in zip(raw_a, raw_b) if x != y]
            comparisons.append({"backend": backend, "case": a["index"], "bytes": len(raw_a),
                                "differingChannels": len(diffs), "maxDifference": max(diffs, default=0)})
    (directory / "pixel-differences.json").write_text(json.dumps(comparisons, indent=2) + "\n")
    print(f"Summarized {len(output)} runs, {len(curves)} unforced heap samples, {len(comparisons)} pixel pairs")


if __name__ == "__main__":
    main()
