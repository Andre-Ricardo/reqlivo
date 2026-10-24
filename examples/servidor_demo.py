#!/usr/bin/env python3
"""Servidor HTTP sintético, local e descartável, usado apenas na validação.

Não contém dados reais. Escuta somente em 127.0.0.1:18080.
Registra requisições de demonstração em JSONL. Use apenas dados sintéticos.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import base64
import gzip
import json
import os
import time

LOG = os.environ.get("SYNTH_LOG", "/tmp/reqlivo-demo.jsonl")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _record(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        rec = {
            "ts": time.time(),
            "method": self.command,
            "path": self.path,
            "headers": {k: v for k, v in self.headers.items()},
            "body_b64": base64.b64encode(body).decode(),
        }
        with open(LOG, "a", encoding="utf-8") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        return body

    def _send(self, status=200, payload=None, headers=None, raw=None):
        data = raw if raw is not None else json.dumps(payload, ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self._record()
        if self.path.startswith("/echo"):
            return self._send(200, {"ok": True, "method": self.command, "path": self.path})
        if self.path.startswith("/status/404"):
            return self._send(404, {"error": "not_found", "synthetic": True})
        if self.path.startswith("/redirect"):
            return self._send(302, {}, {"Location": "/echo?from=redirect"})
        if self.path.startswith("/delay"):
            time.sleep(2)
            return self._send(200, {"delayed": 2})
        if self.path.startswith("/basic"):
            got = self.headers.get("Authorization", "")
            return self._send(200, {"authorization": got, "valid": got == "Basic " + base64.b64encode(b"demo:demo").decode()})
        if self.path.startswith("/cookie/set"):
            return self._send(200, {"cookie_set": True}, {"Set-Cookie": "synthetic=abc123; Path=/; HttpOnly"})
        if self.path.startswith("/cookie/read"):
            return self._send(200, {"cookie": self.headers.get("Cookie")})
        if self.path.startswith("/gzip"):
            body = json.dumps({"gzip": True}).encode()
            z = gzip.compress(body)
            return self._send(200, raw=z, headers={"Content-Encoding": "gzip"})
        if self.path.startswith("/text"):
            data = b"hello synthetic text\nsecond line\n"
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        return self._send(200, {"ok": True, "synthetic": True, "path": self.path})

    def do_POST(self):
        body = self._record()
        try:
            parsed = json.loads(body.decode() or "{}")
        except Exception:
            parsed = {"_raw": body.decode(errors="replace")}
        return self._send(200, {"ok": True, "method": "POST", "received": parsed})

    def do_PUT(self):
        self.do_POST()

    def do_PATCH(self):
        self.do_POST()

    def do_DELETE(self):
        self._record()
        return self._send(200, {"ok": True, "method": "DELETE"})

    def log_message(self, fmt, *args):
        pass


if __name__ == "__main__":
    server = ThreadingHTTPServer(("127.0.0.1", 18080), Handler)
    print("synthetic server on http://127.0.0.1:18080", flush=True)
    server.serve_forever()
