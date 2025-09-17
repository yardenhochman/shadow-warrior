"""
Shadow Warrior Brain Controller - Entry Point

Run the FastAPI application using uvicorn
"""

import uvicorn
from shadow_warrior_brain.core.logging_config import setup_logging, get_logger

logger = get_logger(__name__)


def main():
    """Run the Brain Controller application"""
    setup_logging(debug=True)
    logger.info("Starting Shadow Warrior Brain Controller...")
    uvicorn.run(
        "shadow_warrior_brain.main:app",
        host="0.0.0.0",
        port=8000,
        reload=True
    )


if __name__ == "__main__":
    main()
