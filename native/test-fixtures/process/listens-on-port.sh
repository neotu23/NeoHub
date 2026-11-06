#!/bin/sh
# Listens on the given TCP port (default 4322) and does nothing else.
exec /usr/bin/python3 -c '
import socket, sys, time
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1])))
s.listen(5)
while True:
    time.sleep(1)
' "${1:-4322}"
