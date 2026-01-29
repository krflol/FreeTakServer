import sys
import os
import uuid
import getpass

# Set up paths so we can import FreeTAKServer
current_dir = os.path.dirname(os.path.abspath(__file__))
# Assuming this script is in the root of the repo
sys.path.append(os.path.join(current_dir, 'FreeTAKServer'))

try:
    from FreeTAKServer.core.persistence.DatabaseController import DatabaseController
    from FreeTAKServer.model.SQLAlchemy.system_user import SystemUser
    from FreeTAKServer.model.SQLAlchemy.User import User
    from FreeTAKServer.core.configuration.MainConfig import MainConfig
except ImportError as e:
    print(f"Error importing FreeTAKServer modules: {e}")
    print("Make sure you are running this script from the FreeTakServer root directory and the virtual environment is active.")
    sys.exit(1)

def main():
    print("Initializing Database Controller...")
    # Initialize MainConfig to ensure paths are correct
    config = MainConfig.instance()
    
    # Force DB path if needed, but MainConfig should handle it if env var is set
    # The script run_server.ps1 sets FTS_PERSISTENCE_PATH, so this script should be run in the same env
    
    db = DatabaseController()
    
    print("\n--- Add User Wizard ---")
    print("1. Add System User (for Web UI Login)")
    print("2. Add Standard User (for Data Packages/Groups)")
    
    choice = input("Enter choice (1/2): ").strip()
    
    if choice == '1':
        add_system_user(db)
    elif choice == '2':
        add_standard_user(db)
    else:
        print("Invalid choice.")

def add_system_user(db):
    print("\nAdding System User (Web UI)...")
    username = input("Username: ").strip()
    password = getpass.getpass("Password: ")
    
    if not username or not password:
        print("Username and password cannot be empty.")
        return

    try:
        # Check if user exists
        existing = db.query_systemUser(f'name == "{username}"')
        if existing:
            print(f"User '{username}' already exists.")
            return

        new_user = SystemUser(
            uid=str(uuid.uuid4()),
            name=username,
            password=password,
            token="default_token",
            device_type="webapp"
        )
        
        db.create_systemUser(**new_user.__dict__)
        print(f"System User '{username}' added successfully!")
        
    except Exception as e:
        print(f"Error adding user: {e}")

def add_standard_user(db):
    print("\nAdding Standard User...")
    username = input("Username (Callsign): ").strip()
    
    if not username:
        print("Username cannot be empty.")
        return

    try:
         # Check if user exists
        existing = db.query_user(f'callsign == "{username}"')
        if existing:
            print(f"User '{username}' already exists.")
            return

        # Note: Standard Users in FTS often are created automatically on first connection,
        # or require certificates. This manually adds a DB entry.
        
        # We need a CoT ID usually.
        cot_id = str(uuid.uuid4())
        
        new_user = User(
            uid=cot_id,
            callsign=username,
            CoT_id=cot_id,
            IP="0.0.0.0" # Placeholder
        )
        
        # User creation in DatabaseController is a bit complex, let's try direct add
        db.session.add(new_user)
        db.session.commit()
        
        print(f"Standard User '{username}' added successfully! UID: {cot_id}")
        
    except Exception as e:
        print(f"Error adding user: {e}")
        db.session.rollback()

if __name__ == "__main__":
    main()
