#!/usr/bin/env python3
"""Clica em um diálogo nativo GTK no Xvfb durante a validação desktop.

Os diálogos de confirmação pertencem à janela nativa do Tauri, não à WebView;
por isso não são acessíveis pelo WebDriver. Este utilitário existe somente para
a matriz local automatizada e não faz parte do produto.

Uso:
    python3 scripts/x11-dialog-ok.py --display :99 --x 273 --y 104
"""
import argparse
import ctypes
import os
import time


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--display", default=os.environ.get("DISPLAY", ":99"))
    parser.add_argument("--x", type=int, required=True)
    parser.add_argument("--y", type=int, required=True)
    parser.add_argument("--delay", type=float, default=0.15)
    args = parser.parse_args()

    x11 = ctypes.CDLL("libX11.so.6")
    xtst = ctypes.CDLL("libXtst.so.6")
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XFlush.argtypes = [ctypes.c_void_p]
    xtst.XTestFakeMotionEvent.argtypes = [
        ctypes.c_void_p,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_ulong,
    ]
    xtst.XTestFakeButtonEvent.argtypes = [
        ctypes.c_void_p,
        ctypes.c_uint,
        ctypes.c_int,
        ctypes.c_ulong,
    ]

    display = x11.XOpenDisplay(args.display.encode())
    if not display:
        raise SystemExit(f"não foi possível abrir o display {args.display}")
    time.sleep(max(0.0, args.delay))
    xtst.XTestFakeMotionEvent(display, -1, args.x, args.y, 0)
    xtst.XTestFakeButtonEvent(display, 1, 1, 0)
    xtst.XTestFakeButtonEvent(display, 1, 0, 0)
    x11.XFlush(display)


if __name__ == "__main__":
    main()
