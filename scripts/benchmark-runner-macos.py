#!/usr/bin/env python3
"""Compare native runner CPU cost with owned mock processes; never install apps.

Not a RunCat or energy benchmark. Keep builds/other heavy work paused while it
runs. Both executables use the same synthetic monitor, character and display.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import time


def cpu_seconds(clock):
    days, clock = clock.split("-", 1) if "-" in clock else ("0", clock)
    return int(days) * 86400 + sum(
        float(part) * 60 ** index for index, part in enumerate(reversed(clock.split(":")))
    )


def sample(pid):
    raw = subprocess.check_output(
        ["ps", "-p", str(pid), "-o", "time=,rss="], text=True
    ).strip().split()
    return time.monotonic(), cpu_seconds(raw[0]), int(raw[1])


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None


def window_server_pid():
    rows = subprocess.check_output(["ps", "-wwaxo", "pid=,comm="], text=True).splitlines()
    pids = [int(parts[0]) for row in rows
            if len(parts := row.strip().split(None, 1)) == 2
            and Path(parts[1]).name == "WindowServer"]
    return pids[0] if len(pids) == 1 else None


def stop_owned(child):
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)


def measure(binary, label, index, args, folder):
    log_path = folder / f"{index}-{label}.log"
    with log_path.open("w") as log:
        child = subprocess.Popen(
            [str(binary), "--mock", "--character", args.character, "--display", args.display],
            stdin=subprocess.DEVNULL, stdout=log, stderr=log,
        )
        try:
            time.sleep(args.warmup)
            if child.poll() is not None:
                raise RuntimeError(f"{label} exited during warmup; another mock app may hold the lock. See {log_path}")
            start, cpu_start, rss = sample(child.pid)
            server_error = None
            try:
                server_pid = window_server_pid()
                server_start = sample(server_pid) if server_pid is not None else None
            except (OSError, ValueError, subprocess.SubprocessError) as error:
                server_start, server_error = None, str(error)
            peak_rss = rss
            host_load_start = os.getloadavg()
            while time.monotonic() - start < args.duration:
                time.sleep(max(0, min(1, args.duration - (time.monotonic() - start))))
                if child.poll() is not None:
                    raise RuntimeError(f"{label} exited during measurement; see {log_path}")
                end, cpu_end, rss = sample(child.pid)
                peak_rss = max(peak_rss, rss)
            elapsed = end - start
            server = None
            if server_start is not None:
                try:
                    if window_server_pid() != server_pid:
                        raise RuntimeError("WindowServer identity changed")
                    server_end = sample(server_pid)
                    server = {
                        "pid": server_pid,
                        "one_core_cpu_percent": round(100 * (server_end[1] - server_start[1]) / (server_end[0] - server_start[0]), 3),
                        "scope": "global compositor, includes other desktop activity; not energy or isolated renderer cost",
                    }
                except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                    server_error = str(error)
            return {
                "label": label, "pid": child.pid, "wall_seconds": round(elapsed, 3),
                "cpu_seconds": round(cpu_end - cpu_start, 3),
                "one_core_cpu_percent": round(100 * (cpu_end - cpu_start) / elapsed, 3),
                "peak_rss_mib": round(peak_rss / 1024, 2),
                "host_load_start": host_load_start, "host_load_end": os.getloadavg(),
                "window_server": server, "window_server_error": server_error,
                "log": str(log_path),
            }
        finally:
            stop_owned(child)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--duration", type=float, default=30)
    parser.add_argument("--warmup", type=float, default=5)
    parser.add_argument("--character", choices=["cat", "pushup", "party", "ghost"], default="cat")
    parser.add_argument("--display", choices=["number", "cat", "both"], default="both")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("native status-item measurement requires macOS")
    if not (5 <= args.duration <= 600 and 1 <= args.warmup <= 60):
        parser.error("duration must be 5..600 seconds; warmup must be 1..60 seconds")
    binaries = {name: getattr(args, name).resolve() for name in ["baseline", "candidate"]}
    for binary in binaries.values():
        if not binary.is_file() or not os.access(binary, os.X_OK):
            parser.error(f"not an executable file: {binary}")
    if binaries["baseline"] == binaries["candidate"]:
        parser.error("baseline and candidate must be different files")
    hashes = {name: digest(path) for name, path in binaries.items()}
    config = Path.home() / "Library/Application Support/peterfan/config.toml"
    config_before = digest(config)
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.output.exists():
        parser.error("output already exists; use a fresh evidence path")
    run_folder = args.output.parent / (args.output.stem + "-logs")
    if run_folder.exists():
        parser.error("measurement logs already exist; use a fresh evidence path")
    run_folder.mkdir()
    runs = []
    report = {
        "scope": "macOS native mock app, synthetic varying CPU, ABBA; not real hardware idle or RunCat parity",
        "character": args.character, "display": args.display,
        "binaries": {name: str(path) for name, path in binaries.items()},
        "sha256": hashes, "config_sha256_before": config_before, "runs": runs,
    }
    try:
        for index, label in enumerate(["baseline", "candidate", "candidate", "baseline"]):
            run = measure(binaries[label], label, index, args, run_folder)
            runs.append(run)
            print(json.dumps(run), flush=True)
            if digest(config) != config_before:
                raise RuntimeError("user configuration changed; comparison stopped without overwriting it")
            if any(digest(path) != hashes[name] for name, path in binaries.items()):
                raise RuntimeError("an executable changed during comparison; do not rebuild while measuring")
        medians = {
            label: statistics.median(r["one_core_cpu_percent"] for r in runs if r["label"] == label)
            for label in binaries
        }
        report["median_one_core_cpu_percent"] = medians
        report["relative_reduction_percent"] = (
            round(100 * (1 - medians["candidate"] / medians["baseline"]), 2)
            if medians["baseline"] > 0 else None
        )
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        report["config_preserved"] = digest(config) == config_before
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: value for key, value in report.items() if key != "runs"}, indent=2))


if __name__ == "__main__":
    main()
