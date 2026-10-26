"""Compare catalog import and content indexing time of two builds on generated synthetic inputs."""
import argparse
import json
import pathlib
import platform
import random
import subprocess
import tempfile
import time

p = argparse.ArgumentParser()
p.add_argument("--baseline", required=True, type=pathlib.Path)
p.add_argument("--candidate", required=True, type=pathlib.Path)
p.add_argument("--output", required=True, type=pathlib.Path)
p.add_argument("--rounds", type=int, default=2)
a = p.parse_args()
results = {"platform": platform.platform(), "machine": platform.machine(), "rounds": [],
           "scope": "CLI wall time including process start and exit; CSV import of 100,000 records in 7,000 folders and content scan of 3,000 small UTF-8 text files in 30 folders; unencrypted; warm OS cache; no browser"}
with tempfile.TemporaryDirectory(prefix="allfiles-writes-") as tmp:
    root = pathlib.Path(tmp)
    rng = random.Random(7)
    export = root / "export.csv"
    with export.open("w", encoding="utf-8", newline="") as f:
        f.write("Filename,Size,Date Modified,Disk\n")
        for i in range(100000):
            d = i % 7000
            f.write(f"D:\\Archive\\Project {d // 100:03d}\\Folder {d:04d}\\drawing-{i:06d} kuća.pdf,{1000 + i},2024-01-02 03:04:05,Disc 01\n")
    docs = root / "docs"
    words = ["alpha", "beta", "gamma", "delta", "omega", "kuća", "šuma"]
    for i in range(3000):
        folder = docs / f"dir{i % 30:02d}"
        folder.mkdir(parents=True, exist_ok=True)
        text = f"Dokument {i} kuća garden timber plan " + " ".join(rng.choice(words) for _ in range(400))
        (folder / f"note-{i:05d}.txt").write_text(text, encoding="utf-8")
    steps = [("import_100k", "import", ["--file", str(export), "--label", "Bench"], {"files": 100000, "sources": 1}),
             ("reimport_100k", "import", ["--file", str(export), "--label", "Bench"], {"files": 100000, "sources": 1}),
             ("scan_3000_documents", "scan", ["--root", str(docs), "--label", "Docs", "--content"], {"files": 3000, "documents": 3000}),
             ("rescan_3000_documents", "scan", ["--root", str(docs), "--label", "Docs", "--content"], {"files": 3000, "documents": 3000})]
    for round_number in range(1, a.rounds + 1):
        for name in [["baseline", "candidate"], ["candidate", "baseline"]][(round_number - 1) % 2]:
            binary = getattr(a, name).resolve()
            row = {"round": round_number, "implementation": name, "binary_bytes": binary.stat().st_size, "seconds": {}}
            for label, command, args, expected in steps:
                data = root / f"{round_number}-{name}-{command}"
                data.mkdir(exist_ok=True)
                start = time.perf_counter()
                done = subprocess.run([str(binary), command, "--data", str(data), *args], capture_output=True, text=True, encoding="utf-8")
                elapsed = time.perf_counter() - start
                assert done.returncode == 0, done.stderr[-500:]
                stats = json.loads(done.stdout.strip().splitlines()[-1])
                assert all(stats[key] == value for key, value in expected.items()), (label, stats)
                row["version"] = stats["version"]
                row["seconds"][label] = elapsed
            results["rounds"].append(row)
            print(name, round_number, {k: round(v, 2) for k, v in row["seconds"].items()}, flush=True)
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
