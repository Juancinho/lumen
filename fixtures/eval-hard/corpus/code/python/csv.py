"""Read a CSV export into dictionaries, skipping blank lines."""

import csv

def read_rows(path):
    with open(path, newline="", encoding="utf-8") as f:
        return [row for row in csv.DictReader(f) if any(row.values())]
