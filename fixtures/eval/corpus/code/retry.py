"""HTTP helpers: retry failed requests with exponential backoff."""

import random
import time

import requests


def get_with_retry(url, attempts=5, base_delay=0.5, timeout=10):
    """GET `url`, retrying connection errors and 5xx answers with jittered backoff."""
    for attempt in range(attempts):
        try:
            response = requests.get(url, timeout=timeout)
            if response.status_code < 500:
                return response
        except requests.ConnectionError:
            pass
        delay = base_delay * (2 ** attempt) + random.uniform(0, 0.1)
        time.sleep(delay)
    raise RuntimeError(f"giving up after {attempts} attempts")
