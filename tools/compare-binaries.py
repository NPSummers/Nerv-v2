#!/usr/bin/env python3
"""Time standalone executables only. No compiler/JIT is launched in timed runs."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time


def stats(samples):
    return {"samples_ms": samples, "min_ms": min(samples), "median_ms": statistics.median(samples),
            "mean_ms": statistics.mean(samples), "max_ms": max(samples)}


def cpu_model():
    if platform.system() == "Darwin":
        try:
            return subprocess.check_output(["/usr/sbin/sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip()
        except (OSError, subprocess.CalledProcessError):
            pass
    return platform.processor() or platform.machine()


def validate_heavy(label, stdout):
    lines = stdout.splitlines()
    if label == "Nerv":
        if len(lines) != 2 or lines[-1] != "0":
            raise ValueError("Nerv benchmark must print its result followed by the void-main result 0")
    elif len(lines) != 1:
        raise ValueError("Rust benchmark must print one result")
    result = int(lines[0])
    # sum(j % 22 for j in range(100_000)) plus a random value in 0..10_000.
    if not 1_049_940 <= result < 1_059_940:
        raise ValueError(f"{label} produced an incorrect benchmark result: {result}")


def run_once(label, path, env, show_output=False, heavy=False):
    started = time.perf_counter_ns()
    result = subprocess.run([str(path)], env=env, capture_output=True)
    elapsed = (time.perf_counter_ns() - started) / 1_000_000
    stdout = result.stdout.decode("utf-8").replace("\r\n", "\n")
    if result.returncode:
        raise RuntimeError(f"{label} exited {result.returncode}: {result.stderr.decode('utf-8', errors='replace')}")
    if result.stderr:
        raise RuntimeError(f"{label} wrote unexpected stderr: {result.stderr.decode('utf-8', errors='replace')}")
    if heavy:
        validate_heavy(label, stdout)
    if show_output:
        print(f"  {label} output:\n{stdout}", end="", flush=True)
    return elapsed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nerv-exe", type=Path, required=True)
    parser.add_argument("--rust-exe", type=Path)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--show-program-output", action="store_true")
    parser.add_argument("--heavy-workload", action="store_true")
    parser.add_argument("--json-output", type=Path)
    parser.add_argument("--rustc-version")
    parser.add_argument("--llvm-version")
    parser.add_argument("--cpu-target", choices=["generic", "native"])
    parser.add_argument("--effective-rustflags")
    args = parser.parse_args()
    if args.runs < 1 or args.warmups < 0:
        parser.error("runs must be >= 1 and warmups must be >= 0")
    binaries = {"Nerv": args.nerv_exe.resolve()}
    if args.rust_exe:
        binaries["Rust"] = args.rust_exe.resolve()
    for path in binaries.values():
        if not path.is_file() or not os.access(path, os.X_OK):
            parser.error(f"executable not found or not executable: {path}")
    if len(set(binaries.values())) != len(binaries):
        parser.error("Nerv and Rust must be different executables")
    if args.json_output and args.json_output.resolve() in binaries.values():
        parser.error("JSON output must not overwrite a timed executable")

    env = os.environ.copy()
    for key in ["LLVM_SYS_231_PREFIX", "LLVM_CONFIG_PATH", "DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH", "LD_LIBRARY_PATH"]:
        env.pop(key, None)
    workload = "10,000 × 100,000 = 1,000,000,000 logical inner iterations" if args.heavy_workload else "custom program"
    print(f"Workload: {workload}\nTiming standalone release programs; compilation excluded.", flush=True)
    for label, path in binaries.items():
        print(f"{label}: {path}", flush=True)
    for warmup in range(args.warmups):
        for label, path in binaries.items():
            run_once(label, path, env, heavy=args.heavy_workload)
        print(f"Warmup {warmup + 1}/{args.warmups} complete (not scored).", flush=True)
    samples = {label: [] for label in binaries}
    # Alternate execution order to reduce systematic ordering/thermal bias.
    for run in range(args.runs):
        order = list(binaries) if run % 2 == 0 else list(reversed(binaries))
        print(f"\nRound {run + 1}/{args.runs}:", flush=True)
        for label in order:
            elapsed = run_once(label, binaries[label], env, args.show_program_output, args.heavy_workload)
            samples[label].append(elapsed)
            print(f"  {label}: {elapsed:.3f} ms ({elapsed / 1000:.6f} s)", flush=True)
    measurements = {label: stats(values) for label, values in samples.items()}
    print("\n=== Standalone release comparison ===", flush=True)
    for label, values in measurements.items():
        print(f"{label}: median {values['median_ms']:.3f} ms | avg {values['mean_ms']:.3f} ms | min {values['min_ms']:.3f} ms | max {values['max_ms']:.3f} ms")
    ratio = None
    if "Rust" in measurements:
        ratio = measurements["Nerv"]["median_ms"] / measurements["Rust"]["median_ms"]
        print(f"Nerv / Rust median ratio: {ratio:.3f}x ({ratio:.2f}x slower)" if ratio >= 1 else
              f"Nerv / Rust median ratio: {ratio:.3f}x ({1 / ratio:.2f}x faster)")
    if args.json_output:
        report = {"timestamp_utc": datetime.now(timezone.utc).isoformat(), "platform": platform.platform(),
                  "architecture": platform.machine(), "cpu": cpu_model(), "rustc_version": args.rustc_version,
                  "llvm_version": args.llvm_version, "rustflags": os.environ.get("RUSTFLAGS", ""),
                  "cpu_target": args.cpu_target, "effective_rustflags": args.effective_rustflags,
                  "workload": workload, "runs": args.runs, "warmups": args.warmups,
                  "execution": "standalone", "build_mode": "release", "build_time_included": False,
                  "binaries": {label: str(path) for label, path in binaries.items()},
                  "measurements": measurements, "nerv_over_rust_median": ratio}
        args.json_output.parent.mkdir(parents=True, exist_ok=True)
        args.json_output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(f"Saved report: {args.json_output}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError) as error:
        raise SystemExit(f"Benchmark failed: {error}")
