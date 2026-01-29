import requests
import os

UPLOAD_URL = 'http://127.0.0.1:8081/Marti/api/sync/package'
DOWNLOAD_BASE = 'http://127.0.0.1:8081/Marti/api/sync/package'

def create_dummy_zip():
    filename = 'test_pkg.zip'
    with open(filename, 'wb') as f:
        f.write(b'PK\x03\x04' + b'\x00' * 20 + b'DUMMY_ZIP_CONTENT')
    return filename

def verify_datapackage():
    # 1. Create file
    filename = create_dummy_zip()
    print(f"Created {filename}")

    # 2. Upload
    try:
        with open(filename, 'rb') as f:
            files = {'file': (filename, f)}
            print(f"Uploading to {UPLOAD_URL}...")
            r = requests.post(UPLOAD_URL, files=files)
            print(f"Upload Status: {r.status_code}")
            print(f"Upload Response: {r.text}")
    except Exception as e:
        print(f"Upload Failed: {e}")
        return

    # 3. Download
    download_url = f"{DOWNLOAD_BASE}/{filename}"
    print(f"Downloading from {download_url}...")
    try:
        r = requests.get(download_url)
        if r.status_code == 200:
            print("Download Success! Content Match:", r.content.endswith(b'DUMMY_ZIP_CONTENT'))
        else:
            print(f"Download Failed: {r.status_code}")
    except Exception as e:
        print(f"Download Error: {e}")
    
    # Cleanup
    if os.path.exists(filename):
        os.remove(filename)

if __name__ == "__main__":
    verify_datapackage()
