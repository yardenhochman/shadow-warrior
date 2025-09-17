"""
Shadow Warrior Brain Controller - Entry Point

Run the FastAPI application using uvicorn
"""

import uvicorn


def main():
    """Run the Brain Controller application"""
    print("Starting Shadow Warrior Brain Controller...")
    uvicorn.run(
        "shadow_warrior_brain.main:app",
        host="0.0.0.0",
        port=8000,
        reload=True
    )


if __name__ == "__main__":
    main()
