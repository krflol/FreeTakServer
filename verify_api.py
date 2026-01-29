import urllib.request
import json
import socket
import threading
import time

def check_api():
    try:
        with urllib.request.urlopen('http://127.0.0.1:8080/Manage/HealthCheck') as response:
            status_code = response.getcode()
            data = json.load(response)
            print(f"[API] Status Code: {status_code}")
            print(f"[API] Response: {data}")
    except Exception as e:
        print(f"[API] Error: {e}")

def send_cot():
    host = '127.0.0.1'
    port = 8087
    cot_xml = b'''<?xml version="1.0" standalone="yes"?>
<event version="2.0" uid="TEST-API-CONCURRENCY" type="t-x-c-t" time="2026-01-29T00:00:00Z" start="2026-01-29T00:00:00Z" stale="2026-01-29T00:10:00Z" how="m-g">
    <point lat="0.0" lon="0.0" hae="0.0" ce="9999999" le="9999999"/>
</event>'''
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.connect((host, port))
        s.sendall(cot_xml)
        print("[TCP] Sent CoT Message")
        s.close()
    except Exception as e:
        print(f"[TCP] Error: {e}")

if __name__ == "__main__":
    time.sleep(1) # Give server time to respond if just started
    print("--- Testing API ---")
    check_api()
    print("\n--- Testing TCP ---")
    send_cot()
