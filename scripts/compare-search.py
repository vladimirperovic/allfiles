#!/usr/bin/env python3
"""Sequential, verified SQL-kernel comparison. Uses synthetic catalogs only."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1 << 20), b''):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--go', required=True)
    parser.add_argument('--go-native', required=True)
    parser.add_argument('--rust', required=True)
    parser.add_argument('--data', required=True, type=Path)
    parser.add_argument('--cases', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--iterations', type=int, default=20)
    options = parser.parse_args()
    manifest = json.loads(options.cases.read_text(encoding='utf-8'))
    expected = {case['name']: case for case in manifest['cases']}
    paths = [options.data / name for name in ('catalog.sqlite', 'content.sqlite')]
    before = {path.name: sha256(path) for path in paths}
    binaries = {'go-pure': options.go, 'go-native': options.go_native, 'rust-native': options.rust}
    records = {key: [] for key in binaries}
    # Reverse the second round to reduce any advantage from running last.
    for order in (list(binaries), list(reversed(binaries))):
        for name in order:
            binary = str(Path(binaries[name]).resolve())
            args = [binary, '--data', str(options.data.resolve()), '--cases', str(options.cases.resolve()), '--iterations', str(options.iterations), '--warmup', '3']
            with tempfile.TemporaryDirectory(prefix='allfiles-compare-') as temp:
                rss_path = Path(temp) / 'rss-kib.txt'
                # GNU time reports this individual process's peak RSS, not an
                # accumulated maximum over all earlier child processes.
                result = subprocess.run(['/usr/bin/time', '-f', '%M', '-o', str(rss_path), *args], capture_output=True, text=True)
                if result.returncode != 0:
                    raise RuntimeError(f'{name} exited {result.returncode}:\n{result.stderr.strip()}')
                payload = json.loads(result.stdout)
                if payload['implementation'] != name or payload['iterations'] != options.iterations or payload['warmup'] != 3:
                    raise ValueError(f'{name}: incorrect executable or sampling configuration')
                if payload['sqlite_version'] != '3.53.4':
                    raise ValueError(f'{name}: unexpected SQLite version {payload["sqlite_version"]}')
                actual = payload['results']
                if len(actual) != len(expected) or {row['name'] for row in actual} != set(expected):
                    raise ValueError(f'{name}: missing, duplicate or unexpected cases')
                for row in actual:
                    case = expected[row['name']]
                    if row['count'] != case['expected_count'] or row['ids'] != case['expected_ids']:
                        raise ValueError(f'{name}/{row["name"]}: different results')
                    if any(not math.isfinite(row[field]) or row[field] < 0 for field in ('median_ms', 'p95_ms', 'min_ms')):
                        raise ValueError(f'{name}: invalid timing')
                payload['peak_rss_kib'] = int(rss_path.read_text().strip())
                records[name].append(payload)
    native_go, native_rust = records['go-native'][0], records['rust-native'][0]
    for field in ('sqlite_source_id', 'sqlite_compile_options'):
        if native_go[field] != native_rust[field]:
            raise ValueError(f'Native Go and Rust have different {field}')
    after = {path.name: sha256(path) for path in paths}
    if after != before:
        raise ValueError('A benchmark changed the shared input databases')
    cpu = platform.processor()
    if Path('/proc/cpuinfo').exists():
        cpu = next((line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')), cpu)
    report = {'scope': 'SQL count + ordered ID page, including prepare/finalize; excludes HTTP, document extraction, previews and encryption', 'environment': {'platform': platform.platform(), 'cpu': cpu, 'logical_cpus': os.cpu_count()}, 'iterations_per_round': options.iterations, 'warmup_per_case': 3, 'rounds': 2, 'database_sha256': before, 'database_bytes': {path.name: path.stat().st_size for path in paths}, 'binary_bytes': {key: Path(value).stat().st_size for key, value in binaries.items()}, 'runs': records, 'summary': []}
    for case in manifest['cases']:
        row = {'case': case['name']}
        for name, runs in records.items():
            results = [next(r for r in run['results'] if r['name'] == case['name']) for run in runs]
            row[name] = {'median_ms': statistics.median(r['median_ms'] for r in results), 'max_p95_ms': max(r['p95_ms'] for r in results)}
        report['summary'].append(row)
    options.output.parent.mkdir(parents=True, exist_ok=True)
    options.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, separators=(',', ':')))
    print('\n| SQL workload | Go/current ms | Go/native ms | Rust/native ms |')
    print('|---|---:|---:|---:|')
    for row in report['summary']:
        print(f'| {row["case"]} | ' + ' | '.join(f'{row[name]["median_ms"]:.3f}' for name in binaries) + ' |')
    print('\nPeak RSS (KiB, each complete benchmark process): ' + json.dumps({name: [r['peak_rss_kib'] for r in runs] for name, runs in records.items()}))
    print('Benchmark executable bytes (not complete application sizes): ' + json.dumps(report['binary_bytes']))


if __name__ == '__main__':
    main()
