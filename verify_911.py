import urllib.request
import json
import socket
import ssl
import time
import threading

API_URL = 'http://127.0.0.1:8081'
COT_PORT = 8089

def listen_for_911():
    # Connect to SSL CoT and listen for 911 broadcast
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    
    try:
        with socket.create_connection(('127.0.0.1', COT_PORT)) as sock:
            with context.wrap_socket(sock, server_hostname='127.0.0.1') as ssock:
                print("Listener connected. Waiting for 911...")
                ssock.settimeout(10)
                while True:
                    data = ssock.recv(4096)
                    if not data: break
                    text = data.decode('utf-8', errors='ignore')
                    if 'type="9-1-1"' in text:
                        print(f"\n[SUCCESS] Received 911 Broadcast:\n{text}\n")
                        return
    except Exception as e:
        print(f"Listener failed: {e}")

def trigger_emergency():
    time.sleep(2) # Wait for listener
    print("Triggering Emergency...")
    url = f"{API_URL}/ManageEmergency/postEmergency"
    data = json.dumps({
        "uid": "TEST-911-UID", 
        "name": "TEST-USER", 
        "lat": 1.23, 
        "lon": 4.56
    }).encode('utf-8')
    
    req = urllib.request.Request(url, data=data, headers={'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(req) as res:
            print(f"POST Status: {res.getcode()}")
            print(f"POST Response: {res.read().decode('utf-8')}")
    except Exception as e:
        print(f"POST Failed: {e}")

def run_test():
    t = threading.Thread(target=listen_for_911)
    t.start()
    
    trigger_emergency()
    t.join()

if __name__ == "__main__":
    run_test()
