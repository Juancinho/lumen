"""Retry an HTTP GET with exponential backoff."""

import time
import requests

def get_with_retry(url, attempts=4):
    for i in range(attempts):
        try:
            return requests.get(url, timeout=5)
        except requests.RequestException:
            time.sleep(2 ** i)
    raise RuntimeError("failed")
