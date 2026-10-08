"""Debounce decorator for event handlers."""

import threading

def debounce(seconds):
    def wrap(fn):
        timer = None
        def call(*args):
            nonlocal timer
            if timer:
                timer.cancel()
            timer = threading.Timer(seconds, fn, args)
            timer.start()
        return call
    return wrap
