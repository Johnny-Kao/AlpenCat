import socket
import threading
import signal
import time

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

threading.Thread(target=server_thread, daemon=True).start()

client = socket.create_connection(("127.0.0.1", port))
payload = b"x" * (64 * 1024)

# Bursty, rate-limited loopback traffic. This exercises the network stack while
# deliberately avoiding a CPU-saturating memcpy loop.
target_bytes_per_sec = 8 * 1024 * 1024
burst_seconds = 0.5
idle_seconds = 0.5
bytes_per_burst = int(target_bytes_per_sec * burst_seconds)

try:
    while not stop:
        start = time.perf_counter()
        sent = 0
        while sent < bytes_per_burst and not stop:
            client.sendall(payload)
            sent += len(payload)
            expected = sent / target_bytes_per_sec
            elapsed = time.perf_counter() - start
            if expected > elapsed:
                time.sleep(expected - elapsed)
        time.sleep(idle_seconds)
finally:
    client.close()
    server.close()
