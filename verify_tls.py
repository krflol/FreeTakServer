import socket
import ssl
import time

HOST = '127.0.0.1'
PORT_TLS = 8089

def verify_tls():
    print(f"Connecting to {HOST}:{PORT_TLS} (TLS)...")
    
    # Create a simple context that doesn't verify the self-signed cert hostname strictly
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE 

    try:
        with socket.create_connection((HOST, PORT_TLS)) as sock:
            with context.wrap_socket(sock, server_hostname=HOST) as ssock:
                print(f"SSL Protocol: {ssock.version()}")
                
                # Send CoT
                cot_xml = b'''<?xml version="1.0" standalone="yes"?>
<event version="2.0" uid="TEST-TLS-USER" type="t-x-c-t" time="2026-01-29T00:00:00Z" start="2026-01-29T00:00:00Z" stale="2026-01-29T00:10:00Z" how="m-g">
    <point lat="0.0" lon="0.0" hae="0.0" ce="9999999" le="9999999"/>
</event>'''
                ssock.sendall(cot_xml)
                print("Sent Encrypted CoT")
                time.sleep(1)
                print("Success")
    except Exception as e:
        print(f"TLS Connection Failed: {e}")

if __name__ == "__main__":
    verify_tls()
