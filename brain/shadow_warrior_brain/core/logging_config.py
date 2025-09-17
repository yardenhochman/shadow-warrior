"""
Logging configuration for Shadow Warrior Brain
"""

import logging
import logging.handlers
import sys


def setup_logging(debug: bool = False) -> None:
    """
    Set up logging configuration for the Shadow Warrior Brain application.

    Args:
        debug: Whether to enable debug logging
    """
    log_level = logging.DEBUG if debug else logging.INFO

    # Create root logger
    root_logger = logging.getLogger()
    root_logger.setLevel(log_level)

    # Clear any existing handlers
    root_logger.handlers.clear()

    # Create formatters
    console_formatter = logging.Formatter(
        "%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S"
    )

    file_formatter = logging.Formatter(
        "%(asctime)s [%(levelname)s] %(name)s:%(lineno)d: %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S"
    )

    # Create console handler
    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setLevel(log_level)
    console_handler.setFormatter(console_formatter)

    # Create file handler with rotation
    file_handler = logging.handlers.RotatingFileHandler(
        "shadow_warrior_brain.log",
        maxBytes=10485760,  # 10MB
        backupCount=5
    )
    file_handler.setLevel(log_level)
    file_handler.setFormatter(file_formatter)

    # Add handlers to root logger
    root_logger.addHandler(console_handler)
    # file logging is disabled for now
    # root_logger.addHandler(file_handler)

    # Configure specific loggers
    shadow_warrior_logger = logging.getLogger("shadow_warrior_brain")
    shadow_warrior_logger.setLevel(log_level)

    # Suppress noisy third-party loggers
    logging.getLogger("bleak").setLevel(logging.WARNING)
    logging.getLogger("uvicorn.access").setLevel(logging.WARNING)


def get_logger(name: str) -> logging.Logger:
    """
    Get a logger instance for the given module name.

    Args:
        name: Logger name (typically __name__)

    Returns:
        Logger instance
    """
    return logging.getLogger(f"shadow_warrior_brain.{name}")