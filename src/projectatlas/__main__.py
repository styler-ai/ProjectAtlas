"""
Purpose: Allow `python -m projectatlas` to run the CLI.
"""

from projectatlas.cli import main


if __name__ == "__main__":
    raise SystemExit(main())
