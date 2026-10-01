#!/usr/bin/env python3
"""Reproduce the Generic-object/player-surface observations in report 05.

Uses the existing embed protocol and a disposable in-memory shard. This is an
observation probe, not an acceptance test for a future Mod implementation.
"""

import argparse
import json
from pathlib import Path
import selectors
import subprocess


def main():
    root = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=root / "target/debug/kkscreeps")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    commit = subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
    ).strip()
    process = subprocess.Popen(
        [str(args.binary.resolve()), "embed"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
    )
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    request_id = 0

    def rpc(method, **params):
        nonlocal request_id
        request_id += 1
        request = {"id": request_id, "method": method, "params": params}
        process.stdin.write(json.dumps(request) + "\n")
        process.stdin.flush()
        if not selector.select(timeout=15):
            raise TimeoutError(f"embed RPC timed out: {method}")
        line = process.stdout.readline()
        if not line:
            raise RuntimeError(f"embed exited during {method}")
        reply = json.loads(line)
        if reply.get("id") != request_id or not reply.get("ok"):
            raise RuntimeError(reply)
        return reply["result"]

    try:
        rpc(
            "createShard",
            rooms=[{"name": "W1N1", "owner": "p1", "rcl": 8}],
            players=["p1"],
        )
        object_id = rpc(
            "placeObject",
            room="W1N1",
            type="myobject",
            spec={"pos": [20, 20], "counter": 7, "owner": "p1"},
        )["id"]
        structure_id = rpc(
            "placeObject",
            room="W1N1",
            type="structure",
            spec={
                "pos": [21, 20],
                "structureType": "myfactory",
                "counter": 9,
                "owner": "p1",
                "hits": 200,
            },
        )["id"]
        initial_time = rpc("getGameTime")
        raw_object = rpc("getObject", id=object_id)
        raw_structure = rpc("getObject", id=structure_id)
        code = """(() => {
          const object = Game.getObjectById(OBJECT_ID);
          const structure = Game.getObjectById(STRUCTURE_ID);
          let customFind;
          try {
            customFind = {ok: true, count: Game.rooms.W1N1.find(10000).length};
          } catch (error) {
            customFind = {ok: false, error: String(error)};
          }
          return {
            object: {exists: !!object, counterType: typeof object?.counter,
                     constructor: object?.constructor?.name},
            structure: {exists: !!structure, counterType: typeof structure?.counter,
                        constructor: structure?.constructor?.name},
            customFind,
            internalRegistryType: typeof globalThis.__kkp,
            customClassType: typeof MyObject,
            customCollectionType: typeof Game.customObjects
          };
        })()"""
        code = code.replace("OBJECT_ID", json.dumps(object_id)).replace(
            "STRUCTURE_ID", json.dumps(structure_id)
        )
        player_surface = rpc("runPlayer", handle="p1", code=code)
        host_custom_find = rpc("findInRoom", room="W1N1", type=10000)
        rpc("tick", count=5)
        result = {
            "source_commit": commit,
            "binary": str(args.binary.resolve()),
            "scope": "disposable in-memory embed shard; no installed mod",
            "initial_raw_object": raw_object,
            "initial_raw_structure": raw_structure,
            "player_surface": player_surface,
            "host_custom_find": host_custom_find,
            "elapsed_ticks": rpc("getGameTime") - initial_time,
            "raw_object_after_ticks": rpc("getObject", id=object_id),
        }
        rendered = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
        if args.output:
            args.output.write_text(rendered, encoding="utf-8")
        print(rendered, end="")
    finally:
        selector.close()
        process.stdin.close()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


if __name__ == "__main__":
    main()
