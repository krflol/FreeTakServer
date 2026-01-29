import urllib.request
import json

URL = 'http://127.0.0.1:8081/Manage/Users'

def create_user():
    data = json.dumps({"username": "testuser", "password": "password123"}).encode('utf-8')
    req = urllib.request.Request(URL, data=data, headers={'Content-Type': 'application/json'})
    
    try:
        with urllib.request.urlopen(req) as response:
            print(f"Status: {response.getcode()}")
            print(f"Response: {response.read().decode('utf-8')}")
    except urllib.error.HTTPError as e:
        print(f"Error: {e.code} - {e.read().decode('utf-8')}")
    except Exception as e:
        print(f"Failed: {e}")

if __name__ == "__main__":
    create_user()
