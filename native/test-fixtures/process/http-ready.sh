#!/bin/sh
# Serves /health on the given port: the first request returns 503, then 200.
# Used to prove an http readiness probe waits for the endpoint to actually serve.
exec /usr/bin/python3 -c '
import http.server, sys
port = int(sys.argv[1])
state = {"hits": 0}
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        state["hits"] += 1
        code = 503 if state["hits"] < 2 else 200
        self.send_response(code)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(b"ready")
    def log_message(self, *args):
        pass
http.server.HTTPServer(("127.0.0.1", port), H).serve_forever()
' "${1:-4323}"
