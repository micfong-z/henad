"""A small live display for the long-running sweep scripts.

Redraws a spinner, the run in flight and a bar at the bottom of the terminal, with finished runs
scrolling above. Falls back to one plain line per run whenever stderr is not a terminal, which is
the usual case: a sweep takes hours and gets redirected to a log.

Everything goes to stderr, leaving stdout free.
"""

from __future__ import annotations

import os
import shutil
import sys
import threading
import time

SPINNER = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
BAR_WIDTH = 28

# Set by status, and by nothing else, so a glance down the column reads as a result column.
MARKS = {
    "ok": ("✓", "\x1b[32m"),
    "over_budget": ("!", "\x1b[33m"),
    "timeout": ("!", "\x1b[33m"),
    "oom": ("✗", "\x1b[31m"),
    "error": ("✗", "\x1b[31m"),
    "skipped": ("·", "\x1b[90m"),
}
DIM = "\x1b[90m"
RESET = "\x1b[0m"


def fmt_duration(seconds: float) -> str:
    seconds = int(seconds)
    if seconds >= 3600:
        return f"{seconds // 3600}:{seconds // 60 % 60:02d}:{seconds % 60:02d}"
    return f"{seconds // 60}:{seconds % 60:02d}"


class Progress:
    """Live progress for a run of `total` items, or plain lines when there is no terminal."""

    def __init__(self, total: int, subtitle: str = "", stream=sys.stderr, live: bool | None = None) -> None:
        self.total = total
        self.subtitle = subtitle
        self.stream = stream
        self.live = stream.isatty() if live is None else live
        self.color = self.live and not os.environ.get("NO_COLOR")
        self.done = 0
        self.counts: dict[str, int] = {}
        self._label = ""
        self._note = ""
        self._started_at = time.monotonic()
        self._run_started_at = time.monotonic()
        self._drawn = 0
        self._frame = 0
        self._lock = threading.RLock()
        self._ticker: threading.Thread | None = None
        self._stop = threading.Event()

    # --- lifecycle ---

    def __enter__(self) -> Progress:
        if self.live:
            head = f"{self.total} runs"
            if self.subtitle:
                head += f" · {self.subtitle}"
            self._write(f"{self._dim(head)}\n")
            self._ticker = threading.Thread(target=self._tick, daemon=True)
            self._ticker.start()
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def close(self) -> None:
        self._stop.set()
        if self._ticker is not None:
            self._ticker.join(timeout=1.0)
        with self._lock:
            self._erase()
        counts = ", ".join(f"{n} {status}" for status, n in sorted(self.counts.items()))
        self._write(f"{self.done}/{self.total} in {fmt_duration(time.monotonic() - self._started_at)}")
        self._write(f" · {counts}\n" if counts else "\n")

    # --- the caller's side ---

    def start(self, label: str) -> None:
        with self._lock:
            self._label = label
            self._note = ""
            self._run_started_at = time.monotonic()
            if self.live:
                self._redraw()
            else:
                self._write(f"  run   {label}\n")

    def note(self, text: str) -> None:
        """What the run in flight is doing now, shown only while there is a terminal to show it."""
        with self._lock:
            self._note = text
            if self.live:
                self._redraw()

    def finish(self, label: str, status: str, detail: str = "") -> None:
        with self._lock:
            self.done += 1
            self.counts[status] = self.counts.get(status, 0) + 1
            mark, color = MARKS.get(status, ("·", DIM))
            if self.live:
                self._erase()
                line = f"  {self._paint(mark, color)} {label}"
                if detail:
                    line += f"  {self._dim(detail)}"
                self._write(line + "\n")
                self._label = ""
                self._redraw()
            elif status != "ok":
                self._write(f"        {status}: {detail}\n")

    # --- drawing ---

    def _tick(self) -> None:
        while not self._stop.wait(0.12):
            with self._lock:
                if self._label:
                    self._frame += 1
                    self._redraw()

    def _redraw(self) -> None:
        """Draw the live block.

        Every line is composed and fitted in plain text before any colour goes on. A line wider
        than the terminal would wrap, and the cursor-up that erases the block counts display lines
        rather than the lines it was handed, so a wrapped line corrupts everything below it.
        """
        self._erase()
        width = max(shutil.get_terminal_size((100, 24)).columns, 20)
        lines: list[str] = []

        if self._label:
            spin = SPINNER[self._frame % len(SPINNER)]
            elapsed = fmt_duration(time.monotonic() - self._run_started_at)
            right = f"{self._note} · {elapsed}" if self._note else elapsed
            label = self._ellipsise(self._label, width - len(right) - 6)
            pad = max(1, width - len(label) - len(right) - 4)
            lines.append(f"  {self._paint(spin, '\x1b[36m')} {label}{' ' * pad}{self._dim(right)}")

        counts = f"{self.done}/{self.total}  {100 * self.done // self.total if self.total else 100}%"
        elapsed = fmt_duration(time.monotonic() - self._started_at)
        bar_width = max(6, min(BAR_WIDTH, width - len(counts) - len(elapsed) - 12))
        filled = round(bar_width * self.done / self.total) if self.total else bar_width
        bar = "█" * filled + "░" * (bar_width - filled)
        lines.append(self._dim(f"  {bar}  {counts}  ·  {elapsed}"))

        self._write("".join(line + "\n" for line in lines))
        self._drawn = len(lines)

    @staticmethod
    def _ellipsise(text: str, limit: int) -> str:
        if limit < 4:
            return text[: max(limit, 0)]
        return text if len(text) <= limit else text[: limit - 1] + "…"

    def _erase(self) -> None:
        if self._drawn:
            self._write(f"\x1b[{self._drawn}A\x1b[J")
            self._drawn = 0

    def _paint(self, text: str, color: str) -> str:
        return f"{color}{text}{RESET}" if self.color else text

    def _dim(self, text: str) -> str:
        return self._paint(text, DIM)

    def _write(self, text: str) -> None:
        self.stream.write(text)
        self.stream.flush()
