#!/usr/bin/env python3
"""Read the board's USB console, asserting DTR+RTS like a real terminal.

macOS `cat` does not always raise DTR on open; the firmware only drains its
breadcrumb ring while DTR is asserted, so a plain cat can read nothing at all.

    scripts/console.py [seconds] [port]
"""
import fcntl
import os
import select
import struct
import sys
import termios
import time

SECONDS = float(sys.argv[1]) if len(sys.argv) > 1 else 20.0
PORT = sys.argv[2] if len(sys.argv) > 2 else "/dev/cu.usbmodembench1"

TIOCMGET = 0x4004746A  # _IOR('t', 107, int)  -- Darwin
TIOCMBIS = 0x8004746C  # _IOW('t', 108, int)
TIOCM_DTR = 0x02
TIOCM_RTS = 0x04


def main() -> int:
    fd = os.open(PORT, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    try:
        # raw 8N1
        attrs = termios.tcgetattr(fd)
        iflag, oflag, cflag, lflag, ispeed, ospeed, cc = attrs
        iflag = 0
        oflag = 0
        lflag = 0
        cflag = (cflag | termios.CS8 | termios.CREAD | termios.CLOCAL) & ~termios.PARENB
        attrs = [iflag, oflag, cflag, lflag, termios.B115200, termios.B115200, cc]
        termios.tcsetattr(fd, termios.TCSANOW, attrs)

        # assert DTR + RTS (the firmware gates its drain on DTR)
        bits = struct.pack("i", TIOCM_DTR | TIOCM_RTS)
        fcntl.ioctl(fd, TIOCMBIS, bits)

        deadline = time.time() + SECONDS
        total = 0
        while time.time() < deadline:
            r, _, _ = select.select([fd], [], [], 0.5)
            if not r:
                continue
            try:
                data = os.read(fd, 4096)
            except BlockingIOError:
                continue
            except OSError:
                break
            if not data:
                continue
            total += len(data)
            sys.stdout.write(data.decode("utf-8", "replace"))
            sys.stdout.flush()
        print(f"\n[console.py] {total} bytes in {SECONDS:.0f}s", file=sys.stderr)
        return 0 if total else 2
    finally:
        os.close(fd)


if __name__ == "__main__":
    raise SystemExit(main())
