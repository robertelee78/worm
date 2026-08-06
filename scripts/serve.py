#!/usr/bin/env python3
"""Serve the browser build to this machine and the local network.

    python3 scripts/serve.py [port]        # default 8080, binds 0.0.0.0

Two deliberate differences from `python3 -m http.server -d web`:
  * every response carries Cache-Control: no-store — a phone that visited
    before a wasm rebuild otherwise keeps a stale app.js against a fresh
    engine, which freezes the arena on frame 1 with no error;
  * .wasm is served with the correct MIME type even on Pythons whose
    mimetypes table predates WebAssembly.
"""
import http.server
import mimetypes
import os
import sys

mimetypes.add_type("application/wasm", ".wasm")

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "web")
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8080


class NoStoreHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=ROOT, **kwargs)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


if __name__ == "__main__":
    with http.server.ThreadingHTTPServer(("0.0.0.0", PORT), NoStoreHandler) as srv:
        print(f"serving {os.path.abspath(ROOT)} on http://0.0.0.0:{PORT}", flush=True)
        srv.serve_forever()
