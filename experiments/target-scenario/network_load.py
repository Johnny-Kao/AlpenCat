import socket
import threading
import signal
import sys

stop = False

def on_signal(signum, frame):
    global stop
    stop = True

signal.signal(signal.SIGTERM, on_signal)
signal.signal(signal.SIGINT, on_signal)

server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", 0))
server.listen(1)
port = server.getsockname()[1]
print(f"network_load_port={port}", flush=True)

def server_thread():
    conn, _ = server.accept()
    with conn:
        while not stop:
            data = conn.recv(1 << 20)
            if not data:
                break

t = threading.Thread(target=server_thread, daemon=True)
t.start()

client = socket.create_connection(("127.0.0.1", port))
payload = b"x" * (1 << 20)
try:
    while not stop:
        client.sendall(payload)
finally:
    client.close()
    server.close()
