#!/usr/bin/env python3
"""Convert a CSV export to JSON, keeping only active users."""
import argparse
import csv
import json
import sys


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("input", help="CSV export with name, email, age, active columns")
    p.add_argument("--out", default="out.json", help="where to write the JSON")
    p.add_argument("--verbose", action="store_true", help="print each user to stderr")
    a = p.parse_args()

    # utf-8-sig strips the byte order mark that spreadsheet exports add.
    with open(a.input, newline="", encoding="utf-8-sig") as f:
        rows = [r for r in csv.DictReader(f) if r["active"] == "true"]
    rows.sort(key=lambda r: r["name"])
    for r in rows:
        r["age"] = int(r["age"])
        if a.verbose:
            print(f"{r['name']} ({r['email']})", file=sys.stderr)

    with open(a.out, "w", encoding="utf-8") as f:
        json.dump(rows, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"wrote {len(rows)} users to {a.out}")


if __name__ == "__main__":
    main()
