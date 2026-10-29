"""Regression probes for document extraction; only creates temporary synthetic data."""
import argparse
from contextlib import closing
import json
import pathlib
import sqlite3
import subprocess
import tempfile
import zipfile
import zlib


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=pathlib.Path)
    args = parser.parse_args()
    cases = {
        "baseline.xlsx": ({
            "xl/sharedStrings.xml": b"<sst><si><t>baselineword</t></si></sst>",
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='s'><v>0</v></c></row></worksheet>",
        }, "baselineword"),
        "empty_shared.xlsx": ({
            "xl/sharedStrings.xml": b"<sst><si/><si><t>afteremptyword</t></si></sst>",
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='s'><v>1</v></c></row></worksheet>",
        }, "afteremptyword"),
        "cdata_shared.xlsx": ({
            "xl/sharedStrings.xml": b"<sst><si><t><![CDATA[cdatasharedword]]></t></si></sst>",
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='s'><v>0</v></c></row></worksheet>",
        }, "cdatasharedword"),
        "cdata_inline.xlsx": ({
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='inlineStr'><is><t><![CDATA[cdatainlineword]]></t></is></c></row></worksheet>",
        }, "cdatainlineword"),
        "utf16.docx": ({
            "word/document.xml": "<?xml version='1.0' encoding='UTF-16'?><w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:p><w:r><w:t>utf16word</w:t></w:r></w:p></w:document>".encode("utf-16"),
        }, "utf16word"),
        "escaped.xlsx": ({
            "xl/sharedStrings.xml": b"<sst><si><t>foo_x000D_bar</t></si></sst>",
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='s'><v>0</v></c></row></worksheet>",
        }, "foo\rbar"),
        "empty_shared_preserves_index.xlsx": ({
            "xl/sharedStrings.xml": b"<sst><si/><si><t>correctvalue</t></si><si><t>wrongvalue</t></si></sst>",
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='s'><v>1</v></c></row></worksheet>",
        }, "correctvalue"),
        "escaped_literal_and_surrogate.xlsx": ({
            "xl/sharedStrings.xml": b"<sst><si><t>literal _x005F_x000D_ ocean _xD83C__xDF0A_ end</t></si></sst>",
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='s'><v>0</v></c></row></worksheet>",
        }, "literal _x000D_ ocean \U0001f30a end"),
        "rich_text_escape_boundaries.xlsx": ({
            "xl/sharedStrings.xml": b"<sst><si><r><t>_x005F_</t></r><r><t>x000D_</t></r></si></sst>",
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='s'><v>0</v></c></row></worksheet>",
        }, "_x000D_"),
        "cdata_value.xlsx": ({
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='str'><v><![CDATA[formula<value]]>_x000D_tail</v></c></row></worksheet>",
        }, "formula<value\rtail"),
        "event_boundaries.xlsx": ({
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='inlineStr'><is><t>A&amp;B_x000<![CDATA[D_]]>C_x005F_x000D__xD83C__xDF0A_</t></is></c></row></worksheet>",
        }, "A&B\rC_x000D_\U0001f30a"),
        "utf16be.docx": ({
            "word/document.xml": b"\xfe\xff" + "<?xml version='1.0' encoding='UTF-16'?><w:document xmlns:w='word'><w:p><w:t>Unicode \u010da\u0161a \U0001f30a</w:t></w:p></w:document>".encode("utf-16-be"),
        }, "Unicode \u010da\u0161a \U0001f30a"),
        "utf16be.xlsx": ({
            "xl/sharedStrings.xml": b"\xfe\xff" + "<?xml version='1.0' encoding='UTF-16'?><sst><si/><si><t>Unicode \u010da\u0161a \U0001f30a</t></si></sst>".encode("utf-16-be"),
            "xl/worksheets/sheet1.xml": "<?xml version='1.0' encoding='UTF-16'?><worksheet><row><c t='s'><v>1</v></c></row></worksheet>".encode("utf-16"),
        }, "Unicode \u010da\u0161a \U0001f30a"),
        "malformed_utf16_odd.docx": ({"word/document.xml": b"\xff\xfe<"}, None),
        "malformed_utf16_surrogate.docx": ({"word/document.xml": b"\xfe\xff\xd8\x00"}, None),
        "malformed_utf8.docx": ({"word/document.xml": b"<w:document xmlns:w='word'><w:t>\xff</w:t></w:document>"}, None),
        "malformed_xstring.xlsx": ({
            "xl/worksheets/sheet1.xml": b"<worksheet><row><c t='inlineStr'><is><t>_xD800_</t></is></c></row></worksheet>",
        }, None),
    }
    with tempfile.TemporaryDirectory(prefix="allfiles-audit-extract-") as directory:
        root = pathlib.Path(directory)
        data, docs = root / "data", root / "docs"
        data.mkdir()
        docs.mkdir()
        for name, (parts, _) in cases.items():
            with zipfile.ZipFile(docs / name, "w", zipfile.ZIP_DEFLATED) as archive:
                for part, body in parts.items():
                    archive.writestr(part, body)
        scan = subprocess.run([str(args.binary.resolve()), "scan", "--data", str(data), "--root", str(docs), "--content"], capture_output=True, text=True, encoding="utf-8", timeout=60)
        assert scan.returncode == 0, (scan.stdout, scan.stderr)
        with closing(sqlite3.connect(data / "catalog.sqlite")) as catalog:
            rows = catalog.execute("SELECT id,name,text_status,text_detail FROM files ORDER BY name").fetchall()
        with closing(sqlite3.connect(data / "content.sqlite")) as content:
            texts = {file_id: zlib.decompress(payload).decode("utf-8") for file_id, payload in content.execute("SELECT links.file_id,documents.payload FROM links JOIN documents ON documents.id=links.doc_id")}
        reports = []
        for file_id, name, status, detail in rows:
            expected, actual = cases[name][1], texts.get(file_id, "")
            passed = (status == "error" and not actual) if expected is None else (status == "indexed" and actual == expected)
            reports.append({"name": name, "status": status, "detail": detail, "text": actual, "expected": expected, "pass": passed})
        assert len(reports) == len(cases), "Every synthetic fixture must appear in the catalog"
        print(json.dumps({"cases": reports, "passed": sum(row["pass"] for row in reports), "total": len(reports)}, ensure_ascii=True, indent=2))
        return 0 if all(row["pass"] for row in reports) else 1


if __name__ == "__main__":
    raise SystemExit(main())
