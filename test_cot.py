import socket
import threading
import time

HOST = '127.0.0.1'
PORT = 8087

def listener_client():
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.connect((HOST, PORT))
        print("[Listener] Connected")
        
        while True:
            data = s.recv(4096)
            if not data:
                break
            print(f"[Listener] Received: {data.decode()[:50]}...")
            if b"TEST-USER" in data:
                print("[Listener] SUCCESS: Received Test CoT!")
                break
        s.close()
    except Exception as e:
        print(f"[Listener] Error: {e}")

def sender_client():
    time.sleep(2) # Wait for listener to connect
    cot_xml = b'''<?xml version="1.0" standalone="yes"?>
<event version="2.0" uid="TEST-USER-1" type="t-x-c-t" time="2026-01-28T23:00:00Z" start="2026-01-28T23:00:00Z" stale="2026-01-28T23:10:00Z" how="m-g">
    <point lat="0.0" lon="0.0" hae="0.0" ce="9999999" le="9999999"/>
    <detail>
        <takv version="4.8.0.231" platform="WinTAK-CIV" os="Windows" device="PC"/>
        <contact callsign="TEST-USER" endpoint="*:-1:stcp" phone=""/>
    </detail>
</event>'''

    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.connect((HOST, PORT))
        print("[Sender] Connected")
        s.sendall(cot_xml)
        print("[Sender] Sent XML Data")
        s.close()
    except Exception as e:
        print(f"[Sender] Error: {e}")

if __name__ == "__main__":
    t_listen = threading.Thread(target=listener_client)
    t_listen.start()
    
    sender_client()
    
    t_listen.join(timeout=5)
    print("Test Complete")
