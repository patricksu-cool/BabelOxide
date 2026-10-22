__version__ = "2.9.0"
__major_version__ = "2"
__config_file_version__ = "3"

import os
import sys
from pathlib import Path


def application_directory() -> Path:
    """Return the installed program directory independently of the working directory."""
    if explicit_directory := os.environ.get("PDF2ZH_APP_DIR"):
        directory = Path(explicit_directory).expanduser()
        if not directory.is_absolute():
            raise ValueError("PDF2ZH_APP_DIR must be an absolute path")
        return directory.resolve()

    if getattr(sys, "frozen", False):
        return Path(sys.executable).resolve().parent

    package_parent = Path(__file__).resolve().parent.parent
    if package_parent.name.lower() in {"site-packages", "dist-packages"}:
        return Path(sys.executable).resolve().parent
    return package_parent


# Keep the original file names and TOML format; only the directory changes.
DEFAULT_CONFIG_DIR = application_directory() / "config"
DEFAULT_CONFIG_FILE = DEFAULT_CONFIG_DIR / f"config.v{__config_file_version__}.toml"
WRITE_TEMP_CONFIG_FILE = (
    DEFAULT_CONFIG_DIR / f"config.v{__config_file_version__}.temp.toml"
)
VERSION_DEFAULT_CONFIG_DIR = DEFAULT_CONFIG_DIR / "default"
VERSION_DEFAULT_CONFIG_FILE = VERSION_DEFAULT_CONFIG_DIR / f"{__version__}.toml"

LEGACY_CONFIG_DIR = Path("~/.config/pdf2zh").expanduser()
LEGACY_CONFIG_FILE = LEGACY_CONFIG_DIR / DEFAULT_CONFIG_FILE.name
