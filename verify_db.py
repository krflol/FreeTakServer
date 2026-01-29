import sqlite3
import os

def check_db():
    db_path = r'c:\dev\household\takserver\FreeTakServer\fts-rs\fts.db'
    
    if not os.path.exists(db_path):
        print(f"Database file not found at {db_path}")
        return

    try:
        conn = sqlite3.connect(db_path)
        cursor = conn.cursor()
        
        cursor.execute("SELECT * FROM cot_history")
        rows = cursor.fetchall()
        
        print(f"Found {len(rows)} records in cot_history:")
        for row in rows:
            # Row structure: id, timestamp, uid, type, blob
            print(f"ID: {row[0]}, Time: {row[1]}, UID: {row[2]}, Type: {row[3]}")
            
        conn.close()
    except Exception as e:
        print(f"Error reading database: {e}")

if __name__ == "__main__":
    check_db()
