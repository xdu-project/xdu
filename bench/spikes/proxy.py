#!/usr/bin/env python3
"""Logging HTTP proxy in front of the S3 endpoint, so every request a query makes is
recorded with its Range header and response size.

This is what turns "DuckDB reads less" into a number. DuckDB 1.4.4 has no
duckdb_http_stats() table function, and MinIO does not log per-request by default, so
the accounting has to happen in the path. Each request is appended to PROXY_LOG as one
JSON object; measure.py truncates that file before a query and reads it after.

SigV4 survives the hop because the upstream request is sent with the Host header the
client signed, not the proxy's own. Point the client at 19001 with ENDPOINT
'127.0.0.1:19001' and it authenticates against MinIO on 19000 unchanged.

Usage:  python3 proxy.py &          (then EP=127.0.0.1:19001)
Env:    UPSTREAM (default http://127.0.0.1:19000)
        LISTEN_PORT (default 19001)
        PROXY_LOG (default /tmp/xdu-proxy.log)
"""

import http.server
import json
import os
import socketserver
import urllib.error
import urllib.request

UPSTREAM = os.environ.get("UPSTREAM", "http://127.0.0.1:19000")
LISTEN_PORT = int(os.environ.get("LISTEN_PORT", "19001"))
LOG = os.environ.get("PROXY_LOG", "/tmp/xdu-proxy.log")

# Hop-by-hop headers the proxy must not forward verbatim; Content-Length is recomputed
# because the body is buffered.
SKIP_RESPONSE = {"transfer-encoding", "connection", "content-length"}


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _proxy(self, method):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else None

        req = urllib.request.Request(UPSTREAM + self.path, data=body, method=method)
        for key, value in self.headers.items():
            if key.lower() != "host":
                req.add_header(key, value)
        # Preserve the signed Host, or SigV4 fails upstream.
        req.add_header("Host", self.headers.get("Host"))

        try:
            resp = urllib.request.urlopen(req)
            data, code, headers = resp.read(), resp.status, dict(resp.headers)
        except urllib.error.HTTPError as err:
            data, code, headers = err.read(), err.code, dict(err.headers)

        path, _, query = self.path.partition("?")
        with open(LOG, "a") as handle:
            handle.write(json.dumps({
                "m": method,
                "p": path,
                "q": query,
                "range": self.headers.get("Range"),
                "code": code,
                "bytes": len(data),
            }) + "\n")

        self.send_response(code)
        for key, value in headers.items():
            if key.lower() not in SKIP_RESPONSE:
                self.send_header(key, value)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self._proxy("GET")

    def do_HEAD(self):
        self._proxy("HEAD")

    def do_PUT(self):
        self._proxy("PUT")

    def do_POST(self):
        self._proxy("POST")

    def do_DELETE(self):
        self._proxy("DELETE")

    def log_message(self, *args):
        pass  # the JSON log is the record; stderr noise would drown it


class ThreadingServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True


if __name__ == "__main__":
    print(f"proxy {LISTEN_PORT} -> {UPSTREAM}, logging to {LOG}", flush=True)
    ThreadingServer(("127.0.0.1", LISTEN_PORT), Handler).serve_forever()
