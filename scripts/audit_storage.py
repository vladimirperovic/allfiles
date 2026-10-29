"""Reproduce storage/import edge cases against a built AllFiles executable.

All catalogs and input documents are synthetic TemporaryDirectory fixtures.
This is an audit suite: every case records its expected and actual behavior,
and the process exits nonzero if any expectation fails. It never opens an
existing user catalog. Python's sqlite3 is used only for read-only inspection.
"""
import argparse
import contextlib
import json
import os
import pathlib
import platform
import sqlite3
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    cases = []

    def cli(data, *arguments):
        started = time.perf_counter()
        process = subprocess.run(
            [str(binary), *arguments, "--data", str(data)],
            capture_output=True, text=True, encoding="utf-8", timeout=120,
        )
        return {
            "exit": process.returncode,
            "seconds": round(time.perf_counter() - started, 4),
            "stdout": process.stdout[-2000:],
            "stderr": process.stderr[-2000:],
        }

    def query(data, sql):
        uri = (data / "catalog.sqlite").as_uri() + "?mode=ro"
        with contextlib.closing(sqlite3.connect(uri, uri=True)) as connection:
            return [list(row) for row in connection.execute(sql)]

    def rows(data):
        return query(data, """SELECT s.label,d.path,f.name,f.size
            FROM files f JOIN directories d ON d.id=f.dir_id
            JOIN sources s ON s.id=f.source_id ORDER BY d.path,f.name""")

    def record(name, expected, actual, passed, **details):
        case = {"name": name, "passed": bool(passed), "expected": expected,
                "actual": actual, **details}
        cases.append(case)
        print(("PASS " if passed else "FAIL ") + name, flush=True)

    with tempfile.TemporaryDirectory(prefix="allfiles-storage-audit-") as tmp:
        root = pathlib.Path(tmp)

        def imported(name, text, extension="csv", encoding="utf-8"):
            data = root / name
            data.mkdir()
            source = root / (name + "." + extension)
            source.write_text(text, encoding=encoding)
            return data, source, cli(data, "import", "--file", str(source))

        data, _, result = imported("csv_unicode", "Filename,Size,Disk\n"
                                   "C:\\Archive\\Kuća.txt,12,Disc A\n"
                                   "C:\\Archive\\Other.txt,2,Disc B\n")
        expected = [["Disc A", "C:/Archive", "Kuća.txt", 12],
                    ["Disc B", "C:/Archive", "Other.txt", 2]]
        actual = rows(data)
        record("csv_unicode", expected, actual, result["exit"] == 0 and actual == expected,
               command=result)

        data, _, result = imported("csv_utf16", "Filename,Size,Disk\n"
                                   "C:\\Archive\\Kuća.txt,12,Disc A\n", encoding="utf-16")
        expected = [["Disc A", "C:/Archive", "Kuća.txt", 12]]
        actual = rows(data)
        record("csv_utf16", expected, actual, result["exit"] == 0 and actual == expected,
               command=result)

        data, source, _ = imported("same_media_cleanup", "Filename,Size,Disk\n"
                                  "a.txt,1,Disc\nb.txt,2,Disc\n")
        source.write_text("Filename,Size,Disk\na.txt,3,Disc\n", encoding="utf-8")
        result = cli(data, "import", "--file", str(source))
        expected = [["Disc", "", "a.txt", 3]]
        actual = rows(data)
        record("same_media_stale_cleanup", expected, actual,
               result["exit"] == 0 and actual == expected, command=result)
        source.write_text("Filename,Size,Disk\na.txt,3,Disc\nb.txt,invalid,Disc\n",
                          encoding="utf-8")
        result = cli(data, "import", "--file", str(source))
        actual = rows(data)
        record("invalid_csv_preserves_catalog", expected, actual,
               result["exit"] != 0 and actual == expected, command=result)

        data, _, result = imported("xml_folder_attributes", '<catalog><disk name="Archive">'
            '<folder name="Folder A"><file name="one.txt" size="1"/></folder>'
            '<folder name="Folder B"><file name="one.txt" size="2"/></folder>'
            '</disk></catalog>', "xml")
        expected = [["Archive", "Folder A", "one.txt", 1],
                    ["Archive", "Folder B", "one.txt", 2]]
        actual = rows(data)
        record("xml_folder_attributes", expected, actual,
               result["exit"] == 0 and actual == expected, command=result)

        data, _, result = imported("xml_folder_elements", '<catalog><disk name="Archive">'
            '<folder><name>Folder A</name><file name="one.txt" size="1"/></folder>'
            '<folder><name>Folder B</name><file name="one.txt" size="2"/></folder>'
            '</disk></catalog>', "xml")
        actual = rows(data)
        safe_rejection = result["exit"] != 0 and not actual
        record("xml_folder_elements_do_not_merge_files",
               {"accepted_rows": expected, "or": "reject before mutating the catalog"},
               actual, (result["exit"] == 0 and actual == expected) or safe_rejection,
               command=result)

        data, _, result = imported("xml_media_location", '<catalog><disk><name>Archive</name>'
            '<location>Shelf A</location><folder name="Docs"><file name="one.txt" size="1"/>'
            '</folder></disk></catalog>', "xml")
        actual = query(data, "SELECT label,location FROM sources")
        expected = [["Archive", "Shelf A"]]
        record("xml_media_location", expected, actual,
               result["exit"] == 0 and actual == expected, command=result)

        data, _, result = imported("unc_path", "Filename,Size\n\\\\server\\share\\file.txt,1\n")
        actual = rows(data)
        expected = [["unc_path", "//server/share", "file.txt", 1]]
        record("unc_path_preserves_network_root", expected, actual,
               result["exit"] == 0 and actual == expected, command=result,
               impact="The displayed/copied path loses the UNC root; imported files are already barred from the Open action.")

        data, _, result = imported("leading_space_quote",
            'Filename,Size,Disk\n "a,b.txt",1,Disc\n')
        expected = [["Disc", "", "a,b.txt", 1]]
        actual = rows(data)
        record("leading_space_before_quoted_csv_field", expected, actual,
               result["exit"] == 0 and actual == expected, command=result,
               rationale="Compatibility expectation from the historical Go parser's TrimLeadingSpace=true.")

        for name, count, size in [("max_i64_size", 2, 9223372036854775807),
                                  ("cumulative_100tb_sizes", 100000, 100_000_000_000_000)]:
            data = root / name
            data.mkdir()
            source = root / (name + ".csv")
            with source.open("w", encoding="utf-8", newline="") as stream:
                stream.write("Filename,Size\n")
                for index in range(count):
                    actual_size = 1 if name == "max_i64_size" and index else size
                    stream.write(f"file-{index:06}.bin,{actual_size}\n")
            result = cli(data, "import", "--file", str(source))
            stats = query(data, "SELECT files,bytes,typeof(bytes) FROM catalog_stats")
            reopened = cli(data, "export", "--out", str(root / (name + "-copy")))
            record(name + "_keeps_catalog_reopenable",
                   "Reject unsupported cumulative sizes without making the catalog unopenable",
                   {"stored_stats": stats, "reopen_exit": reopened["exit"]},
                   reopened["exit"] == 0, import_command=result, reopen_command=reopened,
                   fixture={"records": count, "size_per_record": size,
                            "classification": "Adversarial robustness; totals exceed signed 64-bit capacity, not a typical archive."})

        data = root / "early_timestamp"
        data.mkdir()
        docs = root / "dated_documents"
        docs.mkdir()
        document = docs / "old.txt"
        document.write_text("archive document", encoding="utf-8")
        # 1602 is representable by NTFS and avoids Windows' special zero FILETIME.
        timestamp = -11612937600
        try:
            os.utime(document, (timestamp, timestamp))
            actual_timestamp = document.stat().st_mtime
            if abs(actual_timestamp - timestamp) > 1:
                raise OSError("Filesystem did not retain the requested timestamp")
        except OSError as error:
            cases.append({"name": "early_filesystem_timestamp", "skipped": str(error)})
        else:
            result = cli(data, "scan", "--root", str(docs), "--content")
            actual = rows(data)
            record("early_filesystem_timestamp", "Catalog the regular file without aborting the entire scan",
                   {"timestamp": actual_timestamp, "rows": actual},
                   result["exit"] == 0 and len(actual) == 1, command=result)

    passed = sum(case.get("passed") is True for case in cases)
    failed = sum(case.get("passed") is False for case in cases)
    report = {"binary": str(binary), "platform": platform.platform(),
              "scope": "Isolated synthetic CLI imports/scans and read-only SQLite inspection; no user catalogs",
              "summary": {"passed": passed, "failed": failed,
                          "skipped": sum("skipped" in case for case in cases)}, "cases": cases}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["summary"]))
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
